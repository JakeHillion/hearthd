use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Context;
use anyhow::bail;
use anyhow::ensure;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::config::Config;
use super::config::DeviceConfig;
use super::mapper;
use super::protocol::Client;
use super::protocol::Control;
use super::protocol::Snapshot;
use super::protocol::Update;
use super::transport::ManagedTransport;
use crate::engine::Integration;
use crate::engine::IntegrationSender;
use crate::engine::Publisher;
use crate::engine::ToIntegrationMessage;
use crate::matter::*;

const FRESH_FOR: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Gate {
    epoch: u64,
    measured: Option<Instant>,
    ready: bool,
}

impl Gate {
    fn accepts(&self) -> bool { self.ready && self.measured.is_some_and(|time| time.elapsed() < FRESH_FOR) }

    fn close(&mut self) {
        // Invalidating a snapshot also invalidates requests queued against it,
        // even if telemetry recovers without a new Bluetooth connection.
        if self.ready || self.measured.is_some() { self.epoch = self.epoch.wrapping_add(1); }
        self.ready = false;
        self.measured = None;
    }

    fn begin_connection(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.ready = false;
        self.measured = None;
    }
}

struct Queued {
    epoch: u64,
    created: Instant,
    message: ToIntegrationMessage,
}

struct Worker {
    requests: mpsc::Sender<Queued>,
    gate: Arc<Mutex<Gate>>,
    task: JoinHandle<()>,
}

pub struct SolixIntegration {
    config: Config,
    workers: HashMap<LocalKey, Worker>,
    stop: watch::Sender<bool>,
}

impl SolixIntegration {
    pub fn new(config: Config) -> Self { Self { config, workers: HashMap::new(), stop: watch::channel(false).0 } }
}

impl Drop for SolixIntegration {
    fn drop(&mut self) {
        // Request cleanup if the integration is dropped while its runtime is
        // still running. This cannot make the daemon await asynchronous work
        // when the existing engine/runtime itself is being stopped.
        self.stop.send_replace(true);
    }
}

#[async_trait]
impl Integration for SolixIntegration {
    fn name(&self) -> &str { "anker_solix" }

    async fn setup(&mut self, sender: IntegrationSender) -> Result<(), Box<dyn Error + Send>> {
        let publisher = Arc::new(Publisher::new(sender));
        for (name, device) in &self.config.devices {
            let key = LocalKey::from(device.serial.clone());
            let (requests, receiver) = mpsc::channel(8);
            let gate = Arc::new(Mutex::new(Gate::default()));
            let task = tokio::spawn(run_device(name.clone(), device.clone(), publisher.clone(), gate.clone(), receiver, self.stop.subscribe()));
            self.workers.insert(key, Worker { requests, gate, task });
        }
        Ok(())
    }

    async fn handle_message(&mut self, message: ToIntegrationMessage) -> Result<(), Box<dyn Error + Send>> {
        let key = match &message { ToIntegrationMessage::InvokeCommand { key, .. } | ToIntegrationMessage::WriteAttribute { key, .. } => key.clone() };
        let result = (|| -> anyhow::Result<()> {
            let worker = self.workers.get(&key).context("unknown S2000 node")?;
            let gate = worker.gate.lock().map_err(|_| anyhow::anyhow!("device state unavailable"))?;
            ensure!(gate.accepts(), "device disconnected or telemetry stale; command was not queued");
            worker.requests.try_send(Queued { epoch: gate.epoch, created: Instant::now(), message }).map_err(|_| anyhow::anyhow!("device command queue full or closed"))?;
            Ok(())
        })();
        result.map_err(|error| Box::new(std::io::Error::other(error.to_string())) as Box<dyn Error + Send>)
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
        self.stop.send_replace(true);
        for (_, mut worker) in self.workers.drain() {
            if tokio::time::timeout(Duration::from_secs(7), &mut worker.task).await.is_err() {
                worker.task.abort();
                let _ = worker.task.await;
                warn!("S2000 worker exceeded disconnect deadline");
            }
        }
        Ok(())
    }
}

fn control(message: &ToIntegrationMessage, snapshot: &Snapshot) -> anyhow::Result<Control> {
    match message {
        ToIntegrationMessage::InvokeCommand { endpoint_id: mapper::AC_OUTPUT, command: ClusterCommand::OnOff(command), .. } => Ok(Control::Ac(match command { OnOffCommand::On => true, OnOffCommand::Off => false, OnOffCommand::Toggle => !snapshot.ac_on.context("AC output state unknown")? })),
        ToIntegrationMessage::InvokeCommand { endpoint_id: mapper::SETTINGS, command: ClusterCommand::ModeSelect(ModeSelectCommand::ChangeToMode { new_mode }), .. } => Ok(Control::Mode(*new_mode)),
        ToIntegrationMessage::WriteAttribute { endpoint_id: mapper::SETTINGS, write, .. } => {
            let value = write.value.as_u64().context("setting must be an unsigned integer")?;
            ensure!(write.cluster == "SolixSettings", "unsupported settings cluster");
            match write.attribute.as_str() { "charge_cap_watts" => Ok(Control::ChargeCap(value.try_into()?)), "backup_soc" => Ok(Control::Backup(value.try_into()?)), "maximum_soc" => Ok(Control::Maximum(value.try_into()?)), _ => bail!("unknown or read-only S2000 setting") }
        }
        _ => bail!("unsupported S2000 endpoint or command"),
    }
}

#[cfg(target_os = "linux")]
async fn run_device(name: String, device: DeviceConfig, publisher: Arc<Publisher>, gate: Arc<Mutex<Gate>>, mut requests: mpsc::Receiver<Queued>, mut stop: watch::Receiver<bool>) {
    use super::transport::Bluez;
    let mut delay = Duration::from_secs(1);
    loop {
        if *stop.borrow() { break }
        if let Ok(mut gate) = gate.lock() { gate.begin_connection(); }
        while requests.try_recv().is_ok() {}
        let opened = tokio::select! {
            biased;
            _ = stop.changed() => break,
            opened = tokio::time::timeout(Duration::from_secs(15), Bluez::open(&device.adapter, &device.address)) => opened.context("BlueZ access timeout").and_then(|opened| opened),
        };
        let result = match opened {
            Err(error) => Err(error),
            Ok(mut link) => {
                let (authenticated, result) = run_link(&mut link, &name, &device, &publisher, &gate, &mut requests, &mut stop).await;
                if authenticated { delay = Duration::from_secs(1); }
                result
            }
        };
        if *stop.borrow() { break }
        if let Err(error) = result { warn!(serial = %device.serial, error = %format!("{error:#}"), "S2000 unavailable; reconnecting"); }
        tokio::select! { biased; _ = stop.changed() => break, _ = tokio::time::sleep(delay) => {} }
        delay = (delay * 2).min(Duration::from_secs(30));
    }
}

#[cfg(not(target_os = "linux"))]
async fn run_device(_name: String, device: DeviceConfig, _publisher: Arc<Publisher>, _gate: Arc<Mutex<Gate>>, _requests: mpsc::Receiver<Queued>, _stop: watch::Receiver<bool>) {
    warn!(serial = %device.serial, "Anker SOLIX S2000 BLE requires Linux system BlueZ");
}

async fn run_link<T: ManagedTransport>(link: &mut T, name: &str, device: &DeviceConfig, publisher: &Publisher, gate: &Mutex<Gate>, requests: &mut mpsc::Receiver<Queued>, stop: &mut watch::Receiver<bool>) -> (bool, anyhow::Result<()>) {
    let mut authenticated = false;
    let result = tokio::select! {
        biased;
        _ = stop.changed() => Ok(()),
        result = async {
            tokio::time::timeout(Duration::from_secs(25), link.establish()).await.context("GATT setup timeout")??;
            let mut client = Client::new(&mut *link);
            let snapshot = tokio::time::timeout(Duration::from_secs(90), client.authenticate(Some(&device.serial), &device.client_id, false, |_| {})).await.context("S2000 authentication timeout")??;
            authenticated = true;
            serve_session(&mut client, name, &device.serial, snapshot, publisher, gate, requests).await
        } => result,
    };
    if let Ok(mut gate) = gate.lock() { gate.close(); }
    // Keep the link here after cancellation. Disconnect must still run even
    // if the engine's bounded event queue is temporarily blocking withdrawal.
    let key = LocalKey::from(device.serial.clone());
    let (removed, disconnected) = tokio::join!(publisher.remove(&key), link.disconnect());
    if let Err(error) = &disconnected { warn!(serial = %device.serial, %error, "S2000 disconnect failed"); }
    (authenticated, result.and(removed.map_err(anyhow::Error::from)).and(disconnected))
}

async fn serve_session<T: super::transport::Transport>(client: &mut Client<'_, T>, name: &str, serial: &str, mut snapshot: Snapshot, publisher: &Publisher, gate: &Mutex<Gate>, requests: &mut mpsc::Receiver<Queued>) -> anyhow::Result<()> {
    snapshot.check_identity(serial)?;
    ensure!(snapshot.identified() && snapshot.soc.is_some(), "fresh identity-checked battery telemetry is required before publishing");
    let key = LocalKey::from(serial);
    let mut last = Instant::now();
    { let mut gate = gate.lock().map_err(|_| anyhow::anyhow!("device gate poisoned"))?; gate.ready = true; gate.measured = Some(last); }
    publisher.publish(mapper::node(serial, name, &snapshot)).await?;
    info!(serial, "S2000 authenticated telemetry published");
    let mut pending: Option<(Control, Instant)> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            biased;
            _ = tick.tick() => {
                if pending.as_ref().is_some_and(|(_, deadline)| Instant::now() >= *deadline) {
                    if let Some((control, _)) = pending.take() { warn!(serial, ?control, "S2000 command unconfirmed: no matching subsequent telemetry before deadline"); }
                }
                if last.elapsed() >= FRESH_FOR {
                    let was_ready = { let mut gate = gate.lock().map_err(|_| anyhow::anyhow!("device gate poisoned"))?; let ready = gate.ready; gate.close(); ready };
                    if was_ready { warn!(serial, "S2000 battery telemetry stale; withdrawing node"); }
                    publisher.remove(&key).await?;
                }
                ensure!(last.elapsed() < Duration::from_secs(90), "telemetry stopped; reconnecting");
            }
            update = client.next(serial) => {
                match update? {
                    Update::Telemetry(update) => {
                        snapshot.merge(&update);
                        if update.soc.is_some() {
                            last = Instant::now();
                            let mut gate = gate.lock().map_err(|_| anyhow::anyhow!("device gate poisoned"))?; gate.ready = true; gate.measured = Some(last);
                        } else if update.present & 4 != 0 {
                            let mut gate = gate.lock().map_err(|_| anyhow::anyhow!("device gate poisoned"))?; gate.close();
                            warn!(serial, "S2000 battery snapshot invalid; withdrawing node until fresh SoC telemetry");
                        }
                        if pending.as_ref().is_some_and(|(control, _)| control.confirms(&update)) {
                            if let Some((control, _)) = pending.take() { info!(serial, ?control, "S2000 command confirmed by telemetry"); }
                        }
                        let ready = gate.lock().map_err(|_| anyhow::anyhow!("device gate poisoned"))?.accepts();
                        if ready { publisher.publish(mapper::node(serial, name, &snapshot)).await?; } else { publisher.remove(&key).await?; }
                    }
                    Update::Ack { kind, status: code } if code != 0 => {
                        warn!(serial, kind, status = code, "S2000 device rejected request; no application confirmed");
                        if pending.as_ref().is_some_and(|(control, _)| control.ack_kind() == kind) { pending = None; }
                    }
                    _ => {}
                }
            }
            request = requests.recv(), if pending.is_none() => {
                let Some(request) = request else { return Ok(()) };
                let valid = { let gate = gate.lock().map_err(|_| anyhow::anyhow!("device gate poisoned"))?; gate.accepts() && gate.epoch == request.epoch && request.created.elapsed() < Duration::from_secs(5) };
                let action = (|| { ensure!(valid, "queued command expired or belongs to a previous snapshot/connection"); let action = control(&request.message, &snapshot)?; action.validate(&snapshot)?; anyhow::Ok(action) })();
                match action {
                    Ok(action) => {
                        client.write(&action).await?;
                        debug!(serial, control = ?action, "S2000 command sent; waiting for device telemetry confirmation");
                        pending = Some((action, Instant::now() + Duration::from_secs(10)));
                        client.subscribe().await?;
                    }
                    Err(error) => warn!(serial, %error, "S2000 command rejected locally"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
