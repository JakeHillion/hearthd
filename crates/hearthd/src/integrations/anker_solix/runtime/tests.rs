use super::*;
use super::super::protocol::tests::*;
use crate::engine::Event;
use crate::engine::NodeId;
use crate::engine::Stamped;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tracing::instrument::WithSubscriber;

fn request(epoch: u64, watts: u16) -> Queued {
    Queued { epoch, created: Instant::now(), message: ToIntegrationMessage::WriteAttribute { key: LocalKey::from(SERIAL), endpoint_id: mapper::SETTINGS, write: AttributeWrite { cluster: "SolixSettings".into(), attribute: "charge_cap_watts".into(), value: serde_json::json!(watts) } } }
}

fn device_config() -> DeviceConfig {
    DeviceConfig { model: "s2000".into(), adapter: "hci0".into(), address: "00:11:22:33:44:55".into(), serial: SERIAL.into(), client_id: CLIENT_ID.into() }
}

fn publisher(capacity: usize) -> (Publisher, mpsc::Receiver<Stamped>) {
    let (events, reports) = mpsc::channel(capacity);
    (Publisher::new(IntegrationSender::new("anker_solix", events)), reports)
}

fn drain(reports: &mut mpsc::Receiver<Stamped>) -> Vec<Event> {
    let mut events = Vec::new();
    while let Ok(report) = reports.try_recv() { events.push(report.event); }
    events
}

fn announcement(events: &[Event]) -> NodeId {
    let [Event::NodeAdded { node_id, node }] = events else { panic!("expected one announcement: {events:?}") };
    assert_eq!(node.key, LocalKey::from(SERIAL));
    *node_id
}

fn withdrawal(events: &[Event], expected: NodeId) {
    assert!(matches!(events, [Event::NodeRemoved { node_id }] if *node_id == expected), "expected withdrawal for {expected}: {events:?}");
}

async fn wait_for(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() { tokio::time::sleep(Duration::from_millis(1)).await; }
    }).await.expect("mock runtime did not reach the expected observable state");
}

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(bytes); Ok(bytes.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

impl Logs {
    fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
        let logs = self.clone();
        tracing_subscriber::fmt().without_time().with_ansi(false).with_max_level(tracing::Level::DEBUG).with_writer(move || logs.clone()).finish()
    }
    fn text(&self) -> String { String::from_utf8(self.0.lock().unwrap().clone()).unwrap() }
}

#[tokio::test(start_paused = true)]
async fn ack_is_not_confirmation_and_expired_queued_commands_are_not_sent() {
    let mut device = FakeDevice::new(false);
    let record = device.record.clone();
    let mut client = Client::new(&mut device);
    let snapshot = client.authenticate(Some(SERIAL), CLIENT_ID, false, |_| {}).await.unwrap();
    record.lock().unwrap().suppress_readback = true;
    let (publisher, mut reports) = publisher(128);
    let gate = Mutex::new(Gate { epoch: 1, ..Default::default() });
    let (commands, mut requests) = mpsc::channel(8);
    commands.send(request(1, 100)).await.unwrap();
    commands.send(request(1, 200)).await.unwrap();
    let logs = Logs::default();
    tokio::select! {
        result = serve_session(&mut client, "test", SERIAL, snapshot, &publisher, &gate, &mut requests).with_subscriber(logs.subscriber()) => panic!("session ended: {result:?}"),
        _ = async {
            wait_for(|| record.lock().unwrap().writes == 1 && logs.text().contains("waiting for device telemetry confirmation")).await;
            assert_eq!(record.lock().unwrap().writes, 1);
            let announced = drain(&mut reports);
            announcement(&announced);
            let Event::NodeAdded { node, .. } = &announced[0] else { unreachable!() };
            let Cluster::SolixSettings(settings) = &node.endpoints[&mapper::SETTINGS].clusters["SolixSettings"] else { panic!() };
            assert_eq!(settings.charge_cap_watts, Some(1100), "ACK must not publish requested 100 W");
            assert!(logs.text().contains("waiting for device telemetry confirmation"));
            tokio::time::advance(Duration::from_secs(11)).await;
            wait_for(|| logs.text().contains("command unconfirmed") && logs.text().contains("command rejected locally")).await;
            assert_eq!(record.lock().unwrap().writes, 1);
            assert!(drain(&mut reports).is_empty());
            assert!(logs.text().contains("command unconfirmed"));
            assert!(logs.text().contains("command rejected locally"));
        } => {}
    }
}

#[tokio::test(start_paused = true)]
async fn invalid_or_stale_battery_data_withdraws_and_fresh_data_reannounces_same_node() {
    let mut device = FakeDevice::new(false);
    let updates = device.updates.clone();
    let record = device.record.clone();
    let mut client = Client::new(&mut device);
    let snapshot = client.authenticate(Some(SERIAL), CLIENT_ID, false, |_| {}).await.unwrap();
    record.lock().unwrap().suppress_readback = true;
    let (publisher, mut reports) = publisher(256);
    let gate = Mutex::new(Gate { epoch: 2, ..Default::default() });
    let (commands, mut requests) = mpsc::channel(8);
    commands.send(request(1, 100)).await.unwrap();
    let logs = Logs::default();
    tokio::select! {
        result = serve_session(&mut client, "test", SERIAL, snapshot, &publisher, &gate, &mut requests).with_subscriber(logs.subscriber()) => panic!("session ended: {result:?}"),
        _ = async {
            wait_for(|| !reports.is_empty() && logs.text().contains("command rejected locally")).await;
            let id = announcement(&drain(&mut reports));
            assert!(gate.lock().unwrap().accepts());
            assert_eq!(record.lock().unwrap().writes, 0, "previous-session command must not be sent");
            commands.send(request(2, 100)).await.unwrap();
            commands.send(request(2, 200)).await.unwrap();
            wait_for(|| record.lock().unwrap().writes == 1).await;
            assert_eq!(record.lock().unwrap().writes, 1);
            updates.send(vec![0xa5, 5, 4, 26, 2, 255, 100]).unwrap();
            wait_for(|| !reports.is_empty()).await;
            assert!(!gate.lock().unwrap().accepts());
            withdrawal(&drain(&mut reports), id);
            updates.send(vec![0xa6, 3, 4, 10, 0]).unwrap();
            wait_for(|| record.lock().unwrap().received_updates >= 2).await;
            assert!(drain(&mut reports).is_empty(), "unrelated measurements must not reannounce an invalid battery snapshot");
            updates.send(vec![0xa5, 5, 4, 26, 2, 73, 100]).unwrap();
            wait_for(|| !reports.is_empty()).await;
            assert_eq!(announcement(&drain(&mut reports)), id);
            // The queued second request belongs to the invalidated snapshot.
            updates.send(vec![0xa4, 7, 4, 0, 0, 0, 0, 100, 0]).unwrap();
            wait_for(|| logs.text().matches("S2000 command rejected locally").count() == 2).await;
            assert_eq!(record.lock().unwrap().writes, 1);
            drain(&mut reports);
            for _ in 0..7 {
                tokio::time::advance(Duration::from_secs(9)).await;
                let expected = record.lock().unwrap().received_updates + 1;
                updates.send(vec![0xa6, 3, 4, 10, 0]).unwrap();
                wait_for(|| record.lock().unwrap().received_updates == expected).await;
            }
            assert!(!gate.lock().unwrap().accepts());
            withdrawal(&drain(&mut reports), id);
            let expected = record.lock().unwrap().received_updates + 1;
            updates.send(vec![0xa6, 3, 4, 12, 0]).unwrap();
            wait_for(|| record.lock().unwrap().received_updates == expected).await;
            assert!(drain(&mut reports).is_empty());
            updates.send(vec![0xa5, 5, 4, 26, 2, 72, 100]).unwrap();
            wait_for(|| !reports.is_empty()).await;
            assert_eq!(announcement(&drain(&mut reports)), id);
        } => {}
    }
}

#[tokio::test]
async fn concurrent_writes_wait_for_each_matching_telemetry_update() {
    let mut device = FakeDevice::new(false);
    let record = device.record.clone();
    let updates = device.updates.clone();
    let mut client = Client::new(&mut device);
    let snapshot = client.authenticate(Some(SERIAL), CLIENT_ID, false, |_| {}).await.unwrap();
    record.lock().unwrap().suppress_readback = true;
    let (publisher, mut reports) = publisher(128);
    let gate = Mutex::new(Gate { epoch: 1, ..Default::default() });
    let (commands, mut requests) = mpsc::channel(8);
    commands.send(request(1, 100)).await.unwrap(); commands.send(request(1, 200)).await.unwrap();
    let logs = Logs::default();
    tokio::select! {
        result = serve_session(&mut client, "test", SERIAL, snapshot, &publisher, &gate, &mut requests).with_subscriber(logs.subscriber()) => panic!("session ended: {result:?}"),
        _ = async {
            wait_for(|| record.lock().unwrap().writes == 1).await;
            announcement(&drain(&mut reports));
            assert_eq!(record.lock().unwrap().writes, 1);
            updates.send(vec![0xa4, 7, 4, 0, 0, 0, 0, 100, 0]).unwrap();
            wait_for(|| record.lock().unwrap().writes == 2).await;
            assert_eq!(record.lock().unwrap().writes, 2);
            drain(&mut reports);
            updates.send(vec![0xa4, 7, 4, 0, 0, 0, 0, 200, 0]).unwrap();
            wait_for(|| logs.text().matches("command confirmed by telemetry").count() == 2 && !reports.is_empty()).await;
            assert!(drain(&mut reports).iter().any(|event| matches!(event, Event::Report { cluster: Cluster::SolixSettings(settings), .. } if settings.charge_cap_watts == Some(200))));
            assert_eq!(logs.text().matches("command confirmed by telemetry").count(), 2);
        } => {}
    }
}

struct ManagedFake {
    device: FakeDevice,
    disconnected: Arc<AtomicBool>,
    physical: watch::Receiver<bool>,
}

#[async_trait]
impl super::super::transport::Transport for ManagedFake {
    async fn send(&mut self, bytes: &[u8]) -> anyhow::Result<()> { super::super::transport::Transport::send(&mut self.device, bytes).await }
    async fn receive(&mut self) -> anyhow::Result<Vec<u8>> {
        tokio::select! {
            biased;
            _ = self.physical.changed() => bail!("mock physical disconnect"),
            received = super::super::transport::Transport::receive(&mut self.device) => received,
        }
    }
}

#[async_trait]
impl ManagedTransport for ManagedFake {
    async fn establish(&mut self) -> anyhow::Result<()> { Ok(()) }
    async fn disconnect(&mut self) -> anyhow::Result<()> { self.disconnected.store(true, Ordering::SeqCst); Ok(()) }
}

#[tokio::test]
async fn authentication_without_fresh_measurements_never_announces() {
    let (publisher, mut reports) = publisher(128);
    let gate = Mutex::new(Gate::default());
    let (_commands, mut requests) = mpsc::channel(8);
    let device = FakeDevice::new(false);
    let record = device.record.clone();
    record.lock().unwrap().suppress_readback = true;
    let (_physical_tx, physical) = watch::channel(false);
    let disconnected = Arc::new(AtomicBool::new(false));
    let mut link = ManagedFake { device, disconnected: disconnected.clone(), physical };
    let (stop_tx, mut stop) = watch::channel(false);
    let config = device_config();
    let (outcome, ()) = tokio::join!(run_link(&mut link, "test", &config, &publisher, &gate, &mut requests, &mut stop), async {
        wait_for(|| record.lock().unwrap().commands.contains(&0x100)).await;
        assert!(record.lock().unwrap().commands.contains(&0x100));
        assert!(!gate.lock().unwrap().accepts());
        assert!(drain(&mut reports).is_empty());
        stop_tx.send(true).unwrap();
    });
    assert!(!outcome.0);
    outcome.1.unwrap();
    assert!(disconnected.load(Ordering::SeqCst));
    assert!(drain(&mut reports).is_empty());
}

#[tokio::test]
async fn disconnection_and_local_shutdown_withdraw_and_reconnect_keeps_the_node_id() {
    let (publisher, mut reports) = publisher(128);
    let gate = Mutex::new(Gate::default());
    let (_commands, mut requests) = mpsc::channel(8);
    let config = device_config();
    let mut previous = None;
    for physical_disconnect in [true, false] {
        gate.lock().unwrap().begin_connection();
        let (physical_tx, physical) = watch::channel(false);
        let disconnected = Arc::new(AtomicBool::new(false));
        let mut link = ManagedFake { device: FakeDevice::new(false), disconnected: disconnected.clone(), physical };
        let (stop_tx, mut stop) = watch::channel(false);
        let (outcome, id) = tokio::join!(run_link(&mut link, "test", &config, &publisher, &gate, &mut requests, &mut stop), async {
            let added = tokio::time::timeout(Duration::from_secs(5), reports.recv()).await.unwrap().unwrap();
            let id = announcement(&[added.event]);
            if physical_disconnect { physical_tx.send(true).unwrap(); } else { stop_tx.send(true).unwrap(); }
            id
        });
        assert!(outcome.0);
        assert_eq!(outcome.1.is_err(), physical_disconnect);
        assert!(disconnected.load(Ordering::SeqCst));
        assert!(!gate.lock().unwrap().accepts());
        withdrawal(&drain(&mut reports), id);
        if let Some(previous) = previous { assert_eq!(previous, id); }
        previous = Some(id);
    }
}

#[tokio::test]
async fn disconnect_runs_even_when_withdrawal_waits_for_the_engine_queue() {
    let (publisher, mut reports) = publisher(1);
    let gate = Mutex::new(Gate::default());
    let (_commands, mut requests) = mpsc::channel(8);
    let config = device_config();
    let (physical_tx, physical) = watch::channel(false);
    let disconnected = Arc::new(AtomicBool::new(false));
    let mut link = ManagedFake { device: FakeDevice::new(false), disconnected: disconnected.clone(), physical };
    let (_stop_tx, mut stop) = watch::channel(false);
    let (outcome, id) = tokio::join!(run_link(&mut link, "test", &config, &publisher, &gate, &mut requests, &mut stop), async {
        wait_for(|| gate.lock().unwrap().accepts()).await;
        assert!(gate.lock().unwrap().accepts());
        physical_tx.send(true).unwrap();
        wait_for(|| disconnected.load(Ordering::SeqCst)).await;
        assert!(disconnected.load(Ordering::SeqCst), "disconnect must not wait for a full event queue");
        assert!(!gate.lock().unwrap().accepts());
        announcement(&[reports.recv().await.unwrap().event])
    });
    assert!(outcome.1.is_err());
    withdrawal(&drain(&mut reports), id);
}

#[tokio::test]
async fn local_command_guard_rejects_unavailable_and_expired_snapshots() {
    let mut integration = SolixIntegration::new(Config { devices: HashMap::new() });
    let gate = Arc::new(Mutex::new(Gate::default()));
    let (requests, mut received) = mpsc::channel(8);
    integration.workers.insert(LocalKey::from(SERIAL), Worker { requests, gate: gate.clone(), task: tokio::spawn(async {}) });
    assert!(integration.handle_message(request(0, 100).message).await.is_err());
    assert!(received.try_recv().is_err());
    { let mut gate = gate.lock().unwrap(); gate.ready = true; gate.measured = Some(Instant::now()); gate.epoch = 7; }
    integration.handle_message(request(7, 100).message).await.unwrap();
    assert_eq!(received.recv().await.unwrap().epoch, 7);
    gate.lock().unwrap().measured = Some(Instant::now() - FRESH_FOR);
    assert!(integration.handle_message(request(7, 100).message).await.is_err());
    gate.lock().unwrap().close();
    assert!(integration.handle_message(request(7, 100).message).await.is_err());
    assert!(received.try_recv().is_err());
    integration.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_soc_or_identity_never_announces() {
    let snapshot = Snapshot::decode(&telemetry()).unwrap();
    let mut missing_soc = snapshot.clone(); missing_soc.soc = None;
    let mut missing_identity = snapshot.clone(); missing_identity.serial = None;
    let mut wrong_identity = snapshot; wrong_identity.model = Some("other".into());
    for snapshot in [missing_soc, missing_identity, wrong_identity] {
        let (publisher, mut reports) = publisher(16);
        let gate = Mutex::new(Gate::default());
        let (_commands, mut requests) = mpsc::channel(8);
        let mut device = FakeDevice::new(false);
        let mut client = Client::new(&mut device);
        assert!(serve_session(&mut client, "test", SERIAL, snapshot, &publisher, &gate, &mut requests).await.is_err());
        assert!(drain(&mut reports).is_empty());
        assert!(!gate.lock().unwrap().accepts());
    }
}

#[test]
fn read_only_mode_attributes_are_rejected() {
    let write = ToIntegrationMessage::WriteAttribute { key: LocalKey::from(SERIAL), endpoint_id: mapper::SETTINGS, write: AttributeWrite { cluster: "ModeSelect".into(), attribute: "current_mode".into(), value: serde_json::json!(2) } };
    assert!(control(&write, &Snapshot::default()).is_err());
}
