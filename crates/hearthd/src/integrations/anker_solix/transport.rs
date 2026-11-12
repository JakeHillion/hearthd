use async_trait::async_trait;

#[async_trait]
pub trait Transport: Send {
    async fn send(&mut self, bytes: &[u8]) -> anyhow::Result<()>;
    async fn receive(&mut self) -> anyhow::Result<Vec<u8>>;
}

#[async_trait]
pub trait ManagedTransport: Transport {
    async fn establish(&mut self) -> anyhow::Result<()>;
    async fn disconnect(&mut self) -> anyhow::Result<()>;
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::BTreeMap;
    use std::pin::Pin;
    use std::time::Duration;

    use anyhow::Context;
    use anyhow::ensure;
    use bluer::AdapterEvent;
    use bluer::gatt::WriteOp;
    use bluer::gatt::remote::Characteristic;
    use bluer::gatt::remote::CharacteristicWriteRequest;
    use futures::Stream;
    use futures::StreamExt;
    use tokio::time::timeout;
    use uuid::Uuid;

    use super::Transport;

    pub struct Bluez {
        _session: bluer::Session,
        device: bluer::Device,
        owned: bool,
        write: Option<Characteristic>,
        notifications: Option<Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>>,
        events: Option<Pin<Box<dyn Stream<Item = bluer::DeviceEvent> + Send>>>,
    }

    async fn adapter(session: &bluer::Session, name: &str) -> anyhow::Result<bluer::Adapter> {
        ensure!(session.adapter_names().await?.iter().any(|candidate| candidate == name), "Bluetooth adapter {name} does not exist");
        let adapter = session.adapter(name)?;
        ensure!(adapter.is_powered().await?, "Bluetooth adapter {name} is powered off; enable it through the system Bluetooth settings");
        Ok(adapter)
    }

    impl Bluez {
        /// Uses the running system bluetoothd. No adapter power or global
        /// discovery properties are changed.
        pub async fn open(adapter_name: &str, address: &str) -> anyhow::Result<Self> {
            let session = bluer::Session::new().await.context("cannot access system BlueZ; check bluetoothd and system D-Bus permissions")?;
            let adapter = adapter(&session, adapter_name).await?;
            let address = address.parse().context("invalid Bluetooth address")?;
            if !adapter.device_addresses().await?.contains(&address) {
                let mut discovery = Box::pin(adapter.discover_devices().await?);
                timeout(Duration::from_secs(10), async {
                    while let Some(event) = discovery.next().await {
                        if matches!(event, AdapterEvent::DeviceAdded(found) if found == address) { return Ok(()) }
                    }
                    anyhow::bail!("Bluetooth discovery stopped before the target appeared")
                }).await.context("target not found; check range and release other controllers")??;
            }
            let device = adapter.device(address)?;
            Ok(Self { _session: session, device, owned: false, write: None, notifications: None, events: None })
        }

        pub async fn establish(&mut self, service_uuid: Uuid, write_uuid: Uuid, notify_uuid: Uuid) -> anyhow::Result<()> {
            ensure!(!self.device.is_connected().await?, "device already connected; stop hearthd, the phone app or the other controller before using this helper");
            self.events = Some(Box::pin(self.device.events().await?));
            self.owned = true;
            timeout(Duration::from_secs(20), self.device.connect()).await.context("Bluetooth connection timeout; another client may hold the unit")?.context("cannot connect; release the phone app or other controller")?;
            let mut notify = None;
            for service in self.device.services().await? {
                if service.uuid().await? != service_uuid { continue }
                for characteristic in service.characteristics().await? {
                    let uuid = characteristic.uuid().await?;
                    if uuid == write_uuid { self.write = Some(characteristic.clone()); }
                    if uuid == notify_uuid { notify = Some(characteristic); }
                }
            }
            ensure!(self.write.is_some(), "S2000 write characteristic missing");
            let notify = notify.context("S2000 notification characteristic missing")?;
            self.notifications = Some(Box::pin(notify.notify().await?));
            Ok(())
        }

        pub async fn disconnect(&mut self) -> anyhow::Result<()> {
            self.notifications = None;
            self.events = None;
            self.write = None;
            if self.owned {
                self.owned = false;
                timeout(Duration::from_secs(5), self.device.disconnect()).await.context("Bluetooth disconnect timeout")??;
            }
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl Transport for Bluez {
        async fn send(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
            let write = self.write.as_ref().context("Bluetooth transport not established")?;
            let request = CharacteristicWriteRequest { op_type: WriteOp::Command, ..Default::default() };
            timeout(Duration::from_secs(5), write.write_ext(bytes, &request)).await.context("Bluetooth write timeout")??;
            Ok(())
        }

        async fn receive(&mut self) -> anyhow::Result<Vec<u8>> {
            loop {
                let notifications = self.notifications.as_mut().context("Bluetooth transport not established")?;
                let events = self.events.as_mut().context("Bluetooth connection monitor missing")?;
                tokio::select! {
                    biased;
                    event = events.next() => match event {
                        Some(bluer::DeviceEvent::PropertyChanged(bluer::DeviceProperty::Connected(false))) | None => anyhow::bail!("Bluetooth device disconnected or BlueZ disappeared"),
                        _ => {}
                    },
                    bytes = notifications.next() => return bytes.context("device disconnected or stopped notifications; another client may have taken the connection"),
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        ensure!(timeout(Duration::from_secs(3), self.device.is_connected()).await.context("BlueZ connection monitor timeout")??, "Bluetooth device disconnected");
                    }
                }
            }
        }
    }

    #[async_trait::async_trait]
    impl super::ManagedTransport for Bluez {
        async fn establish(&mut self) -> anyhow::Result<()> { self.establish(crate::integrations::anker_solix::protocol::SERVICE, crate::integrations::anker_solix::protocol::WRITE, crate::integrations::anker_solix::protocol::NOTIFY).await }
        async fn disconnect(&mut self) -> anyhow::Result<()> { Bluez::disconnect(self).await }
    }

    #[derive(Debug, Clone)]
    pub struct Candidate {
        pub address: String,
        pub name: String,
    }

    pub async fn scan(adapter_name: &str, service: Uuid, matches_name: fn(&str) -> bool) -> anyhow::Result<Vec<Candidate>> {
        let session = bluer::Session::new().await.context("cannot access system BlueZ; check bluetoothd and D-Bus permissions")?;
        let adapter = adapter(&session, adapter_name).await?;
        // BlueZ reference-counts discovery clients. Dropping this stream
        // releases only our discovery session, leaving other clients alone.
        let mut discovery = Box::pin(adapter.discover_devices_with_changes().await?);
        let mut candidates = BTreeMap::new();
        let result = timeout(Duration::from_secs(10), async {
            while let Some(event) = discovery.next().await {
                if let AdapterEvent::DeviceAdded(address) = event {
                    let device = adapter.device(address)?;
                    // Nearby unrelated devices can vanish while properties
                    // are queried; this must not abort the whole scan.
                    let Ok(name) = device.name().await else { continue };
                    let Ok(services) = device.uuids().await else { continue };
                    let Ok(rssi) = device.rssi().await else { continue };
                    let name = name.unwrap_or_default();
                    let services = services.unwrap_or_default();
                    if rssi.is_some() && matches_name(&name) && services.contains(&service) {
                        candidates.insert(address, Candidate { address: address.to_string(), name });
                    }
                }
            }
            anyhow::Ok(())
        }).await;
        if let Ok(result) = result { result?; }
        Ok(candidates.into_values().collect())
    }
}

#[cfg(target_os = "linux")]
pub use linux::*;
