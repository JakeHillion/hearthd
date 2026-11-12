use std::collections::HashMap;
use std::time::Duration;

use anyhow::bail;
use anyhow::ensure;
use tokio::time::Instant;

pub const NEGOTIATION: [u8; 3] = [3, 0, 1];
pub const SESSION: [u8; 3] = [3, 0, 15];
const MAX_BODY: usize = 4096;

pub fn frame(pattern: [u8; 3], command: u16, body: &[u8]) -> anyhow::Result<Vec<u8>> {
    ensure!(body.len() <= MAX_BODY, "outbound body exceeds receive budget");
    let mut frame = vec![0xff, 9];
    frame.extend_from_slice(&((body.len() + 10) as u16).to_le_bytes());
    frame.extend_from_slice(&pattern);
    frame.extend_from_slice(&command.to_be_bytes());
    frame.extend_from_slice(body);
    frame.push(frame.iter().fold(0, |sum, byte| sum ^ byte));
    Ok(frame)
}

struct Run {
    started: Instant,
    parts: Vec<Option<Vec<u8>>>,
}

#[derive(Default)]
pub struct Receiver {
    runs: HashMap<([u8; 3], u16), Run>,
}

pub struct Message {
    pub pattern: [u8; 3],
    /// Encryption/fragment flags removed; reply bit remains.
    pub kind: u16,
    pub ciphertext: Vec<u8>,
}

impl Receiver {
    pub fn accept(&mut self, frame: &[u8]) -> anyhow::Result<Option<Message>> {
        self.runs.retain(|_, run| run.started.elapsed() < Duration::from_secs(5));
        ensure!((10..=1024).contains(&frame.len()), "invalid frame size");
        ensure!(frame[..2] == [0xff, 9], "invalid frame magic");
        ensure!(u16::from_le_bytes([frame[2], frame[3]]) as usize == frame.len(), "invalid frame length");
        ensure!(frame.iter().fold(0, |sum, byte| sum ^ byte) == 0, "frame checksum mismatch");
        let pattern = [frame[4], frame[5], frame[6]];
        ensure!([NEGOTIATION, [3, 1, 1], SESSION, [3, 1, 15], [3, 1, 17]].contains(&pattern), "unknown receive channel");
        let command = u16::from_be_bytes([frame[7], frame[8]]);
        ensure!(command & 0x4000 != 0, "unencrypted message rejected");
        let kind = command & 0x3fff;
        let payload = &frame[9..frame.len() - 1];
        let ciphertext = if command & 0x8000 == 0 {
            payload.to_vec()
        } else {
            let (&descriptor, bytes) = payload.split_first().ok_or_else(|| anyhow::anyhow!("missing fragment descriptor"))?;
            let index = (descriptor >> 4) as usize;
            let count = (descriptor & 15) as usize;
            ensure!(count != 0 && index != 0 && index <= count, "invalid fragment index/count");
            let key = (pattern, kind);
            if index == 1 {
                if self.runs.get(&key).is_some_and(|run| run.parts.len() == count && run.parts[0].as_deref() == Some(bytes)) { return Ok(None) }
                ensure!(self.runs.len() < 8 || self.runs.contains_key(&key), "too many incomplete messages");
                self.runs.insert(key, Run { started: Instant::now(), parts: vec![None; count] });
            }
            let run = self.runs.get_mut(&key).ok_or_else(|| anyhow::anyhow!("fragment arrived without first fragment"))?;
            if run.parts.len() != count {
                self.runs.remove(&key);
                bail!("contradictory fragment totals");
            }
            if let Some(previous) = &run.parts[index - 1] {
                ensure!(previous == bytes, "conflicting fragment duplicate");
                return Ok(None);
            }
            run.parts[index - 1] = Some(bytes.to_vec());
            ensure!(run.parts.iter().flatten().map(Vec::len).sum::<usize>() <= MAX_BODY, "assembled body exceeds budget");
            if run.parts.iter().any(Option::is_none) { return Ok(None) }
            self.runs.remove(&key).unwrap().parts.into_iter().flatten().flatten().collect()
        };
        ensure!(ciphertext.len() >= 16 && ciphertext.len() <= MAX_BODY, "invalid authenticated body size");
        Ok(Some(Message { pattern, kind, ciphertext }))
    }
}

pub fn tlvs(mut bytes: &[u8]) -> anyhow::Result<HashMap<u8, &[u8]>> {
    let mut values = HashMap::new();
    while !bytes.is_empty() {
        ensure!(bytes.len() >= 2, "truncated TLV header");
        let tag = bytes[0];
        let length = bytes[1] as usize;
        ensure!(bytes.len() >= length + 2, "truncated TLV value");
        ensure!(values.insert(tag, &bytes[2..length + 2]).is_none(), "duplicate TLV tag");
        bytes = &bytes[length + 2..];
    }
    Ok(values)
}
