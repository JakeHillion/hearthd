mod crypto;
mod telemetry;
mod wire;
#[cfg(test)]
pub(super) mod tests;

use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::ensure;
use tokio::time::Instant;
use tokio::time::timeout;
use uuid::Uuid;

use super::transport::Transport;
use crypto::Cipher;
use crypto::Exchange;
pub use telemetry::Snapshot;
use wire::NEGOTIATION;
use wire::SESSION;
use wire::tlvs;

pub const ADVERTISED_SERVICE: Uuid = Uuid::from_u128(0x0000ff0900001000800000805f9b34fb);
pub const SERVICE: Uuid = Uuid::from_u128(0x8c850001030241c5b46ecf057c562025);
pub const WRITE: Uuid = Uuid::from_u128(0x8c850002030241c5b46ecf057c562025);
pub const NOTIFY: Uuid = Uuid::from_u128(0x8c850003030241c5b46ecf057c562025);

pub fn timestamp() -> anyhow::Result<u32> { Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().try_into()?) }

fn timestamp_tlv(time: u32) -> Vec<u8> { let mut bytes = vec![0xa1, 4]; bytes.extend_from_slice(&time.to_le_bytes()); bytes }
fn identity_tlv(bytes: &mut Vec<u8>, identity: &str) { bytes.extend_from_slice(&[0xa2, identity.len() as u8]); bytes.extend_from_slice(identity.as_bytes()); }
fn session_body(setting: &[u8], time: u32) -> Vec<u8> {
    let mut bytes = vec![0xa1, 1, 0x21]; bytes.extend_from_slice(setting); bytes.extend_from_slice(&[0xfe, 4]); bytes.extend_from_slice(&time.to_le_bytes()); bytes
}

fn success(body: &[u8], stage: u16) -> anyhow::Result<&[u8]> {
    let (&status, tail) = body.split_first().context("missing response status")?;
    ensure!(status == 0, "protocol stage {stage:04x} rejected with status {status:02x}");
    Ok(tail)
}

pub struct Client<'a, T: Transport> {
    link: &'a mut T,
    cipher: Cipher,
    receiver: wire::Receiver,
    granted: bool,
    enrol_pending: bool,
}

pub enum Update {
    Telemetry(Snapshot),
    Ack { kind: u16, status: u8 },
    Other,
}

impl<'a, T: Transport> Client<'a, T> {
    pub fn new(link: &'a mut T) -> Self { Self { link, cipher: Cipher::negotiation(), receiver: wire::Receiver::default(), granted: false, enrol_pending: false } }

    async fn send(&mut self, pattern: [u8; 3], command: u16, bytes: &[u8]) -> anyhow::Result<()> {
        let encrypted = self.cipher.encrypt(bytes)?;
        self.link.send(&wire::frame(pattern, command, &encrypted)?).await
    }

    async fn receive(&mut self) -> anyhow::Result<(u16, [u8; 3], Vec<u8>)> {
        loop {
            let bytes = self.link.receive().await?;
            if let Some(message) = self.receiver.accept(&bytes)? {
                let plain = self.cipher.decrypt(&message.ciphertext)?;
                return Ok((message.kind, message.pattern, plain));
            }
        }
    }

    async fn reply(&mut self, expected: u16) -> anyhow::Result<Vec<u8>> {
        timeout(Duration::from_secs(15), async {
            loop {
                let (kind, pattern, bytes) = self.receive().await?;
                if kind == expected {
                    ensure!(pattern == NEGOTIATION || expected == 0x827, "negotiation reply on unexpected channel");
                    return Ok(bytes)
                }
                if self.enrol_pending && kind == 0x827 && bytes.first() == Some(&0) && [NEGOTIATION, [3, 1, 1], [3, 1, 15], [3, 1, 17]].contains(&pattern) { self.granted = true; }
            }
        }).await.context("protocol reply timeout; device disconnected or another client holds it")?
    }

    async fn exchange(&mut self, command: u16, bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.send(NEGOTIATION, command, bytes).await?;
        self.reply((command & 0x3fff) | 0x800).await
    }

    pub async fn authenticate(&mut self, expected_serial: Option<&str>, client_id: &str, enrol: bool, mut prompt: impl FnMut(&str)) -> anyhow::Result<Snapshot> {
        let time = timestamp()?;
        let base = timestamp_tlv(time);
        let first = self.exchange(0x4001, &base).await?;
        ensure!(tlvs(success(&first, 0x4801)?)?.get(&0xa1).copied() == Some(&[1][..]), "unsupported connection stage");
        let mut request = base.clone(); request.extend_from_slice(&[0xa3, 1, 0x20, 0xa4, 2, 0, 0xf0]);
        let capability = self.exchange(0x4003, &request).await?;
        let capability = tlvs(success(&capability, 0x4803)?)?;
        ensure!(capability.get(&0xa1).copied() == Some(&[2][..]), "unsupported capability stage");
        let capacity = capability.get(&0xa2).context("missing granted frame capacity")?;
        ensure!(capacity.len() == 2 && (253..=1024).contains(&u16::from_le_bytes((*capacity).try_into().unwrap())), "unsupported frame capacity");
        ensure!(capability.get(&0xa3).is_some_and(|value| value.len() == 1 && value[0] & 0x40 != 0) && capability.get(&0xa5).copied() == Some(&[2][..]), "device does not support encrypted ECDH method 2");
        let info = self.exchange(0x4029, &base).await?;
        let info = tlvs(success(&info, 0x4829)?)?;
        ensure!(info.get(&0xa1).copied() == Some(&[3][..]), "unsupported device-info stage");
        let serial = std::str::from_utf8(info.get(&0xa4).context("missing device serial")?)?.to_string();
        ensure!(!serial.is_empty() && serial.len() <= 32 && serial.bytes().all(|byte| byte.is_ascii_alphanumeric()), "invalid device serial");
        ensure!(expected_serial.is_none_or(|expected| expected == serial), "device-info serial does not match intended unit");
        // The six-byte MAC is validated structurally. Full equality with
        // the BLE address has not been verified on this AS220; the serial
        // and authorized model/serial telemetry establish intended identity.
        ensure!(info.get(&0xa5).is_some_and(|mac| mac.len() == 6), "invalid device-info MAC width");
        let mut request = base.clone(); request.extend_from_slice(&[0xa3, 1, 0x20, 0xa4, 2, 0xfd, 0, 0xa5, 1, 0x44, 0xa6, 1, 2]);
        let selected = self.exchange(0x4005, &request).await?; success(&selected, 0x4805)?;
        let exchange = Exchange::new();
        let mut request = vec![0xa1, 64]; request.extend_from_slice(&exchange.public());
        let key = self.exchange(0x4021, &request).await?;
        let key = tlvs(success(&key, 0x4821)?)?;
        self.cipher = exchange.finish(key.get(&0xa1).context("missing device public key")?)?;
        let mut request = base.clone();
        if enrol { identity_tlv(&mut request, client_id); }
        // These timezone bytes reproduce the proven AS220 negotiation. Their
        // mismatch with GMT0 is documented; no schedule control is exposed.
        request.extend_from_slice(&[0xa3, 4, 0x40, 0x38, 0, 0, 0xa5, 4, b'G', b'M', b'T', b'0']);
        let configured = self.exchange(0x4022, &request).await?; success(&configured, 0x4822)?;
        let mut request = base.clone(); identity_tlv(&mut request, client_id);
        let authorization = self.exchange(0x4027, &request).await?;
        let status = *authorization.first().context("missing authorization status")?;
        ensure!(status == 0 || status == 9, "client authorization rejected with status {status:02x}");
        let mut deadline = Instant::now() + Duration::from_secs(60);
        if status == 9 {
            ensure!(enrol, "existing client ID is not authorized; check the configured identity or explicitly run enrol");
            self.enrol_pending = true;
            if let Some(window) = tlvs(&authorization[1..])?.get(&0xa1) {
                ensure!(window.len() == 2, "invalid authorization window");
                let seconds = u16::from_le_bytes((*window).try_into().unwrap());
                ensure!(seconds > 0, "authorization window already expired");
                deadline = deadline.min(Instant::now() + Duration::from_secs(u64::from(seconds)));
            }
            let mut registration = base; identity_tlv(&mut registration, client_id);
            registration.extend_from_slice(&[0xa3, serial.len() as u8]); registration.extend_from_slice(serial.as_bytes()); registration.extend_from_slice(&[0xa4, 1, 0]);
            self.send(NEGOTIATION, 0x4023, &registration).await?;
            prompt(&serial);
            let registration = self.reply(0x823).await?;
            let registration = tlvs(success(&registration, 0x4823)?)?;
            if let Some(repeated) = registration.get(&0xa1) { ensure!(*repeated == serial.as_bytes(), "registration serial mismatch"); }
        } else {
            self.granted = true;
        }
        // Authorization, not the unproven compatibility 4023 finalization,
        // gates the first subscription. Reuse never sends registration.
        self.subscribe().await?;
        let mut retry = tokio::time::interval(Duration::from_secs(1));
        retry.tick().await;
        let mut snapshot = Snapshot::default();
        timeout(deadline.saturating_duration_since(Instant::now()), async {
            loop {
                tokio::select! {
                    _ = retry.tick() => self.subscribe().await?,
                    received = self.receive() => {
                        let (kind, pattern, bytes) = received?;
                        if kind == 0x827 {
                            let status = *bytes.first().context("missing grant status")?;
                            ensure!(status == 0 || status == 9, "authorization grant rejected with status {status:02x}");
                            if status == 0 && [NEGOTIATION, [3, 1, 1], [3, 1, 15], [3, 1, 17]].contains(&pattern) {
                                self.granted = true;
                                self.subscribe().await?;
                            }
                        } else if self.granted && (kind == 0x900 || kind == 0x421) {
                            if kind == 0x900 && bytes.first() != Some(&0) { continue }
                            let update = Snapshot::decode(&bytes)?;
                            update.check_identity(&serial)?;
                            snapshot.merge(&update);
                            if snapshot.identified() && update.soc.is_some() { self.enrol_pending = false; return Ok(snapshot) }
                        }
                    }
                }
            }
        }).await.context("timed out waiting for authorization and identity-checked telemetry; keep the helper connected and press Power once when prompted")?
    }

    pub async fn subscribe(&mut self) -> anyhow::Result<()> { self.send(SESSION, 0x4100, &session_body(&[0xa2, 2, 4, 1], timestamp()?)).await }

    pub async fn write(&mut self, control: &Control) -> anyhow::Result<()> {
        ensure!(self.granted, "client is not authorized");
        let (command, setting) = control.encoding();
        self.send(SESSION, command, &session_body(&setting, timestamp()?)).await
    }

    pub async fn next(&mut self, serial: &str) -> anyhow::Result<Update> {
        let (kind, _, bytes) = self.receive().await?;
        if kind == 0x900 || kind == 0x421 {
            if kind == 0x900 && bytes.first() != Some(&0) { return Ok(Update::Ack { kind, status: *bytes.first().context("empty status reply")? }) }
            let update = Snapshot::decode(&bytes)?;
            update.check_identity(serial)?;
            return Ok(Update::Telemetry(update));
        }
        if [0x901, 0x903, 0x890].contains(&kind) { return Ok(Update::Ack { kind, status: *bytes.first().context("empty setting acknowledgment")? }) }
        Ok(Update::Other)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control { Ac(bool), Mode(u8), ChargeCap(u16), Backup(u8), Maximum(u8) }

impl Control {
    pub fn validate(&self, snapshot: &Snapshot) -> anyhow::Result<()> {
        match *self {
            Self::Ac(_) => {}
            Self::Mode(value) => ensure!([0, 2].contains(&value), "only standard (0) and self-consumption (2) are supported"),
            Self::ChargeCap(value) => {
                let maximum = snapshot.max_charge_cap_watts.context("device has not reported its maximum charge cap")?;
                ensure!(value >= 50 && value <= maximum, "charge cap must be 50..={maximum} W; zero is not a charger-off command");
            }
            Self::Backup(value) => {
                ensure!((5..=100).contains(&value), "backup SoC must be 5..=100 percent");
                ensure!(snapshot.max_soc.is_some_and(|maximum| value <= maximum), "backup must not exceed the reported maximum SoC");
            }
            Self::Maximum(value) => {
                ensure!((80..=100).contains(&value), "maximum SoC must be 80..=100 percent");
                ensure!(snapshot.backup_soc.is_some_and(|backup| value >= backup), "maximum SoC must not be below the reported backup SoC");
            }
        }
        Ok(())
    }

    fn encoding(&self) -> (u16, Vec<u8>) {
        match *self {
            Self::Ac(on) => (0x4101, vec![0xa2, 2, 1, u8::from(on)]),
            Self::Mode(mode) => (0x4090, vec![0xa2, 2, 1, mode]),
            Self::ChargeCap(watts) => { let [lo, hi] = watts.to_le_bytes(); (0x4101, vec![0xa4, 3, 2, lo, hi]) }
            Self::Backup(soc) => (0x4090, vec![0xa5, 2, 1, soc]),
            Self::Maximum(soc) => (0x4103, vec![0xaa, 2, 1, soc]),
        }
    }

    pub fn ack_kind(&self) -> u16 { (self.encoding().0 & 0x3fff) | 0x800 }

    pub fn confirms(&self, update: &Snapshot) -> bool {
        match *self { Self::Ac(value) => update.ac_on == Some(value), Self::Mode(value) => update.mode == Some(value), Self::ChargeCap(value) => update.charge_cap_watts == Some(value), Self::Backup(value) => update.backup_soc == Some(value), Self::Maximum(value) => update.max_soc == Some(value) }
    }
}
