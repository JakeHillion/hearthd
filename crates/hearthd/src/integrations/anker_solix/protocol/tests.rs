use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use super::*;
use super::wire::frame;

fn hex(value: &str) -> Vec<u8> { value.as_bytes().as_chunks::<2>().0.iter().map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect() }

fn decode_record(record: &[u8]) -> anyhow::Result<Snapshot> {
    let mut bytes = vec![0xa1, 1, 0x31]; bytes.extend_from_slice(record); Snapshot::decode(&bytes)
}

#[test]
fn independent_static_and_session_vectors_match_exact_bytes() {
    let static_plain = hex("a10404030201");
    let encrypted = Cipher::negotiation().encrypt(&static_plain).unwrap();
    assert_eq!(encrypted, hex("0a823bd1e43befe90dd68ff51e02c4af25d4e0a0c9f6"));
    assert_eq!(frame(NEGOTIATION, 0x4001, &encrypted).unwrap(), hex("ff09200003000140010a823bd1e43befe90dd68ff51e02c4af25d4e0a0c9f676"));
    let shared: [u8; 32] = std::array::from_fn(|index| index as u8);
    let session = Cipher::from_shared(&shared);
    let plain = session_body(&[0xa4, 3, 2, 0x4c, 4], 0x01020304);
    assert_eq!(plain, hex("a10121a403024c04fe0404030201"));
    let encrypted = session.encrypt(&plain).unwrap();
    assert_eq!(encrypted, hex("652f220b0c4dfaebe9d959f6c52646ca2c96ff28b055b4de4fc8ef44fd7e"));
    assert_eq!(frame(SESSION, 0x4101, &encrypted).unwrap(), hex("ff09280003000f4101652f220b0c4dfaebe9d959f6c52646ca2c96ff28b055b4de4fc8ef44fd7e1c"));
    assert_eq!(session.decrypt(&encrypted).unwrap(), plain);
}

#[test]
fn integrity_and_invalid_public_points_fail_closed() {
    let cipher = Cipher::negotiation();
    let mut encrypted = cipher.encrypt(b"valid plaintext").unwrap();
    encrypted[0] ^= 1;
    // A repaired outer checksum cannot repair an authentication failure.
    let bytes = frame(NEGOTIATION, 0x4801, &encrypted).unwrap();
    let message = wire::Receiver::default().accept(&bytes).unwrap().unwrap();
    assert!(cipher.decrypt(&message.ciphertext).is_err());
    assert!(cipher.decrypt(&[0; 15]).is_err());
    assert!(Exchange::new().finish(&[0; 64]).is_err());
    assert!(Exchange::new().finish(&[4; 65]).is_err());
    let mut bad = bytes.clone(); bad[2] ^= 1;
    assert!(wire::Receiver::default().accept(&bad).is_err());
    let mut bad = bytes; *bad.last_mut().unwrap() ^= 1;
    assert!(wire::Receiver::default().accept(&bad).is_err());
}

#[tokio::test(start_paused = true)]
async fn fragments_are_bounded_authenticated_and_connection_scoped() {
    let encrypted = Cipher::negotiation().encrypt(b"telemetry with ff09 inside the data").unwrap();
    let fragment = |descriptor, bytes: &[u8]| { let mut body = vec![descriptor]; body.extend_from_slice(bytes); frame([3, 1, 15], 0xc900, &body).unwrap() };
    let first = fragment(0x12, &encrypted[..10]);
    let second = fragment(0x22, &encrypted[10..]);
    let mut receiver = wire::Receiver::default();
    assert!(receiver.accept(&first).unwrap().is_none());
    assert!(receiver.accept(&first).unwrap().is_none());
    let message = receiver.accept(&second).unwrap().unwrap();
    assert_eq!(message.ciphertext, encrypted);
    assert!(wire::Receiver::default().accept(&second).is_err());
    let mut receiver = wire::Receiver::default(); receiver.accept(&first).unwrap();
    assert!(receiver.accept(&fragment(0x23, &encrypted[10..])).is_err());
    for descriptor in [0, 0x02, 0x20, 0x32] { assert!(wire::Receiver::default().accept(&fragment(descriptor, &encrypted)).is_err()); }
    let mut receiver = wire::Receiver::default(); receiver.accept(&first).unwrap();
    tokio::time::advance(Duration::from_secs(6)).await;
    assert!(receiver.accept(&second).is_err());
}

fn typed(tag: u8, data: &[u8], bytes: &mut Vec<u8>) { bytes.extend_from_slice(&[tag, (data.len() + 1) as u8, 4]); bytes.extend_from_slice(data); }
pub const SERIAL: &str = "TEST1234567890123";
pub const CLIENT_ID: &str = "11111111-2222-4333-8444-555555555555";

pub fn telemetry() -> Vec<u8> {
    let mut bytes = vec![0, 0xa1, 1, 0x31];
    let mut identity = vec![6, 0x20, SERIAL.len() as u8]; identity.extend_from_slice(SERIAL.as_bytes()); identity.extend_from_slice(&[0, 5]); identity.extend_from_slice(b"AS220");
    bytes.extend_from_slice(&[0xa2, identity.len() as u8]); bytes.extend_from_slice(&identity);
    typed(0xa3, &[2, 0, 0, 0, 0x4c, 4], &mut bytes);
    let mut settings = [0; 30]; settings[4..6].copy_from_slice(&1100u16.to_le_bytes()); settings[23] = 95; settings[24] = 1;
    typed(0xa4, &settings, &mut bytes);
    typed(0xa5, &[26, 2, 73, 100], &mut bytes);
    typed(0xa6, &[231, 0, 0x3f, 5, 0, 0, 15, 0], &mut bytes);
    typed(0xa7, &[1, 224, 0, 0, 224, 0], &mut bytes);
    typed(0xaa, &[1, 7, 0], &mut bytes);
    typed(0xd9, &[0, 0, 10, 95, 1], &mut bytes);
    bytes
}

#[test]
fn telemetry_identity_units_missing_and_invalid_fields_are_distinct() {
    let snapshot = Snapshot::decode(&telemetry()).unwrap();
    snapshot.check_identity(SERIAL).unwrap();
    assert!(snapshot.check_identity("WRONG").is_err());
    assert!(snapshot.identified());
    assert_eq!(snapshot.soc, Some(73));
    assert_eq!(snapshot.remaining_seconds, Some(5400));
    assert_eq!(snapshot.grid_watts, Some(1343));
    assert_eq!(snapshot.total_watts, Some(231));
    assert_eq!(snapshot.ac_watts, Some(224));
    assert_eq!(snapshot.usb_watts, Some(7));
    assert_eq!(snapshot.max_charge_cap_watts, Some(1100));
    assert!(decode_record(&[0xa5, 9, 4, 26]).is_err());
    assert!(decode_record(&[0xa5, 2, 1, 26]).is_err());
    assert!(Snapshot::decode(&[0xa5, 5, 4, 26, 2, 73, 100]).is_err());
    assert!(wire::tlvs(&hex("a402024c04")).is_err());
    let mut updated = snapshot.clone();
    let unrelated = decode_record(&[0xa6, 3, 4, 10, 0]).unwrap(); updated.merge(&unrelated);
    assert_eq!(updated.soc, Some(73));
    let invalid = decode_record(&[0xa5, 5, 4, 26, 2, 255, 100]).unwrap(); updated.merge(&invalid);
    assert_eq!(updated.soc, None);
    assert_eq!(updated.temperature, Some(26));
    let short = decode_record(&[0xa5, 1, 4]).unwrap(); updated.merge(&short);
    assert_eq!(updated.temperature, None);
    assert_eq!(updated.flow, None);
    let mut wrong = snapshot; wrong.model = Some("A1783".into()); assert!(wrong.check_identity(SERIAL).is_err());
}

#[test]
fn settings_validate_native_limits_and_confirm_only_from_readback() {
    let snapshot = Snapshot::decode(&telemetry()).unwrap();
    for action in [Control::Ac(false), Control::Mode(0), Control::Mode(2), Control::ChargeCap(50), Control::ChargeCap(1100), Control::Backup(73), Control::Maximum(95)] { action.validate(&snapshot).unwrap(); }
    for action in [Control::Mode(1), Control::Mode(3), Control::ChargeCap(0), Control::ChargeCap(49), Control::ChargeCap(1101), Control::Backup(4), Control::Backup(96), Control::Maximum(79), Control::Maximum(101)] { assert!(action.validate(&snapshot).is_err()); }
    assert!(!Control::ChargeCap(100).confirms(&snapshot));
    assert!(!Control::ChargeCap(100).confirms(&Snapshot::default()));
    assert!(Control::ChargeCap(1100).confirms(&snapshot));
    assert_eq!(Control::Backup(10).encoding(), (0x4090, vec![0xa5, 2, 1, 10]));
}

#[derive(Default)]
pub struct Record {
    pub commands: Vec<u16>,
    pub registration_count: usize,
    pub configured_identity: bool,
    pub bad_authorization: bool,
    pub suppress_readback: bool,
    pub writes: usize,
    pub received_updates: usize,
}

pub struct FakeDevice {
    cipher: Cipher,
    incoming: VecDeque<Vec<u8>>,
    enrol: bool,
    pub record: Arc<Mutex<Record>>,
    pub updates: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    injected: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
}

impl FakeDevice {
    pub fn new(enrol: bool) -> Self { let (updates, injected) = tokio::sync::mpsc::unbounded_channel(); Self { cipher: Cipher::negotiation(), incoming: VecDeque::new(), enrol, record: Arc::new(Mutex::new(Record::default())), updates, injected } }
    fn reply(&mut self, pattern: [u8; 3], command: u16, body: &[u8]) { self.incoming.push_back(frame(pattern, command, &self.cipher.encrypt(body).unwrap()).unwrap()); }
}

#[async_trait::async_trait]
impl Transport for FakeDevice {
    async fn send(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let message = wire::Receiver::default().accept(bytes)?.unwrap();
        let plain = self.cipher.decrypt(&message.ciphertext)?;
        self.record.lock().unwrap().commands.push(message.kind);
        match message.kind {
            1 => self.reply(NEGOTIATION, 0x4801, &[0, 0xa1, 1, 1]),
            3 => self.reply(NEGOTIATION, 0x4803, &[0, 0xa1, 1, 2, 0xa2, 2, 0xfd, 0, 0xa3, 1, 0x44, 0xa4, 1, 1, 0xa5, 1, 2]),
            0x29 => { let mut body = vec![0, 0xa1, 1, 3, 0xa4, SERIAL.len() as u8]; body.extend_from_slice(SERIAL.as_bytes()); body.extend_from_slice(&[0xa5, 6, 0, 0x11, 0x22, 0x33, 0x44, 0x55]); self.reply(NEGOTIATION, 0x4829, &body); }
            5 => self.reply(NEGOTIATION, 0x4805, &[0]),
            0x21 => {
                let key = Exchange::new();
                let mut reply = vec![0, 0xa1, 64]; reply.extend_from_slice(&key.public());
                let session = key.finish(tlvs(&plain)?.get(&0xa1).unwrap())?;
                self.reply(NEGOTIATION, 0x4821, &reply); self.cipher = session;
            }
            0x22 => {
                self.record.lock().unwrap().configured_identity = tlvs(&plain)?.contains_key(&0xa2);
                self.reply(NEGOTIATION, 0x4822, &[0]);
            }
            0x27 => {
                let bad = self.record.lock().unwrap().bad_authorization;
                self.reply(NEGOTIATION, 0x4827, if bad { &[4] } else if self.enrol { &[9, 0xa1, 2, 30, 0] } else { &[0] });
            }
            0x23 => {
                self.record.lock().unwrap().registration_count += 1;
                self.reply(NEGOTIATION, 0x4823, &[0]);
                self.reply([3, 1, 1], 0x4827, &[0]);
            }
            0x100 => {
                let suppress = self.record.lock().unwrap().suppress_readback;
                if !suppress { self.reply([3, 1, 15], 0x4900, &telemetry()); }
            }
            0x101 | 0x103 | 0x90 => {
                self.record.lock().unwrap().writes += 1;
                self.reply([3, 1, 15], message.kind | 0x4800, &[0]);
            }
            other => anyhow::bail!("unexpected mock command {other:04x}"),
        }
        Ok(())
    }

    async fn receive(&mut self) -> anyhow::Result<Vec<u8>> {
        match self.incoming.pop_front() { Some(frame) => Ok(frame), None => {
            let update = self.injected.recv().await.context("mock transport disconnected")?;
            self.record.lock().unwrap().received_updates += 1;
            let mut routed = vec![0xa1, 1, 0x31]; routed.extend_from_slice(&update);
            frame([3, 1, 15], 0x4421, &self.cipher.encrypt(&routed)?)
        } }
    }
}

#[tokio::test]
async fn new_registration_is_sent_once_and_reuse_never_binds() {
    for enrol in [false, true] {
        let mut device = FakeDevice::new(enrol);
        let record = device.record.clone();
        let mut client = Client::new(&mut device);
        let mut prompts = 0;
        let snapshot = client.authenticate(Some(SERIAL), CLIENT_ID, enrol, |_| prompts += 1).await.unwrap();
        assert_eq!(snapshot.soc, Some(73));
        assert_eq!(prompts, usize::from(enrol));
        let record = record.lock().unwrap();
        assert_eq!(record.registration_count, usize::from(enrol));
        assert_eq!(record.configured_identity, enrol);
        assert_eq!(&record.commands[..7], &[1, 3, 0x29, 5, 0x21, 0x22, 0x27]);
    }
}

#[tokio::test]
async fn handshake_success_without_authorization_or_telemetry_is_not_success() {
    let mut device = FakeDevice::new(false);
    device.record.lock().unwrap().bad_authorization = true;
    assert!(Client::new(&mut device).authenticate(Some(SERIAL), CLIENT_ID, false, |_| panic!("reuse must not prompt")).await.is_err());
    let mut device = FakeDevice::new(true);
    assert!(Client::new(&mut device).authenticate(Some(SERIAL), CLIENT_ID, false, |_| panic!("reuse must not enroll")).await.is_err());
    let mut device = FakeDevice::new(false);
    assert!(Client::new(&mut device).authenticate(Some("WRONG"), CLIENT_ID, true, |_| panic!("identity must be checked before registration")).await.is_err());
    assert_eq!(device.record.lock().unwrap().registration_count, 0);
}

#[tokio::test(start_paused = true)]
async fn authorized_connection_without_measurements_times_out() {
    let mut device = FakeDevice::new(false);
    device.record.lock().unwrap().suppress_readback = true;
    assert!(Client::new(&mut device).authenticate(Some(SERIAL), CLIENT_ID, false, |_| {}).await.is_err());
}
