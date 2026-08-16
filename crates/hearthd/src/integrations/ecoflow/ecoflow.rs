//! The EcoFlow integration: session lifecycle, message routing and command
//! dispatch.
//!
//! Everything device-specific lives in `super::wave3`; everything
//! cloud-specific in `super::cloud`. This module wires the two together and
//! presents them to the engine.
//!
//! # Shape of the runtime
//!
//! Devices are declared in configuration, so every node exists from startup
//! and is announced immediately, before any telemetry arrives. Its attributes
//! are null until the device reports. That keeps a node's shape stable rather
//! than having endpoints appear as data trickles in.
//!
//! A single background task owns the session: authenticate, connect,
//! subscribe, ask for a snapshot, then pump messages until the session ends,
//! then back off and start again. Credentials are re-fetched on every attempt
//! because neither the bearer token nor the MQTT credentials advertise their
//! expiry, and a refused connection is the only signal that they have lapsed.

use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use rand::Rng;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::cloud::auth::EcoFlowApi;
use super::cloud::session::Backoff;
use super::cloud::session::STALE_AFTER;
use super::cloud::topics;
use super::cloud::transport::Message;
use super::cloud::transport::Transport;
use super::config::Config;
use super::wave3::codec;
use super::wave3::codec::ConfigWrite;
use super::wave3::matter as wave3_matter;
use super::wave3::state::DeviceState;
use super::wave3::wire;
use crate::engine::FromIntegrationMessage;
use crate::engine::FromIntegrationSender;
use crate::engine::Integration;
use crate::engine::NodeId;
use crate::engine::NodeIdAllocator;
use crate::engine::ToIntegrationMessage;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::Endpoint;
use crate::matter::EndpointId;
use crate::matter::Node;

/// Integration name reported to the engine.
pub const INTEGRATION_NAME: &str = "ecoflow";

/// Sequence numbers are a fresh random value per command rather than a
/// counter: that is what the app does and what the firmware accepts.
const SEQ_MIN: u32 = 10;
const SEQ_MAX: u32 = 999;

/// How often the watchdog looks for devices that have stopped reporting.
const STALENESS_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// One declared device and everything known about it.
struct Device {
    node_id: NodeId,
    entity_id: String,
    name: String,
    serial: String,
    state: DeviceState,
    /// Last endpoint map handed to the engine, so only genuine changes are
    /// reported.
    published: HashMap<EndpointId, Endpoint>,
    /// Whether this device's silence has already been reported, so the
    /// watchdog logs the transition rather than every check.
    stale_reported: bool,
}

impl Device {
    fn node(&self) -> Node {
        Node {
            entity_id: self.entity_id.clone(),
            integration: INTEGRATION_NAME.to_string(),
            name: Some(self.name.clone()),
            endpoints: self.published.clone(),
        }
    }
}

/// A device from configuration, before the engine has assigned it a node id.
///
/// Ids come from the engine's allocator, which only arrives at `setup`, so
/// what configuration determines is kept apart from what the engine assigns.
struct DeclaredDevice {
    entity_id: String,
    name: String,
    serial: String,
}

#[derive(Default)]
struct Inner {
    devices: HashMap<NodeId, Device>,
    /// Reverse index: device serial to node id.
    serial_to_node: HashMap<String, NodeId>,
}

type SharedInner = Arc<Mutex<Inner>>;

pub struct EcoFlowIntegration<A: EcoFlowApi, T: Transport> {
    api: Arc<A>,
    transport: Arc<Mutex<T>>,
    config: Config,
    inner: SharedInner,
    /// Configured devices awaiting a node id, drained at `setup`.
    declared: Vec<DeclaredDevice>,
    /// Fixed for the process: a reconnect then replaces our own previous
    /// session rather than accumulating sessions.
    client_uuid: String,
    /// Account id of the live session, or `None` while disconnected.
    ///
    /// Command topics are user-scoped, so this is what makes a command
    /// sendable; its absence is how the integration knows it cannot send one.
    user_id: Arc<Mutex<Option<String>>>,
    to_engine: Option<FromIntegrationSender>,
    session_task: Option<JoinHandle<()>>,
    watchdog_task: Option<JoinHandle<()>>,
}

impl<A: EcoFlowApi + 'static, T: Transport + 'static> EcoFlowIntegration<A, T> {
    pub fn new(api: A, transport: T, config: &Config) -> Self {
        // Sorted so that ids are drawn in a deterministic order and a restart
        // does not reshuffle them.
        let mut names: Vec<&String> = config.devices.keys().collect();
        names.sort();

        let declared: Vec<DeclaredDevice> = names
            .into_iter()
            .map(|name| {
                let device_config = &config.devices[name];
                DeclaredDevice {
                    entity_id: format!("climate.{name}"),
                    name: device_config.name.clone().unwrap_or_else(|| name.clone()),
                    serial: device_config.serial.clone(),
                }
            })
            .collect();

        Self {
            api: Arc::new(api),
            transport: Arc::new(Mutex::new(transport)),
            config: config.clone(),
            inner: Arc::new(Mutex::new(Inner::default())),
            declared,
            client_uuid: topics::random_uuid_hex(),
            user_id: Arc::new(Mutex::new(None)),
            to_engine: None,
            session_task: None,
            watchdog_task: None,
        }
    }

    /// Watch for devices that have gone quiet.
    ///
    /// A long-lived clean MQTT session says nothing about whether any
    /// particular device is alive — only an arriving frame does. hearthd's
    /// Matter model has no availability flag, so a stale device cannot be
    /// marked unavailable and its last-known attributes stay visible. Logging
    /// the transition is what can be done today; the alternative, silently
    /// serving indefinitely-old values as current, is worse.
    async fn staleness_watchdog(inner: SharedInner) {
        loop {
            tokio::time::sleep(STALENESS_CHECK_INTERVAL).await;

            let now = std::time::Instant::now();
            let mut guard = inner.lock().await;

            for device in guard.devices.values_mut() {
                let stale = device.state.is_stale(now, STALE_AFTER);

                if stale && !device.stale_reported {
                    if device.state.has_data() {
                        warn!(
                            "EcoFlow device {} has not reported for {STALE_AFTER:?}; its state may be out of date",
                            device.serial
                        );
                    } else {
                        // Only meaningful once a session is up: until then
                        // there is nothing to distinguish a wrong serial from
                        // a device that simply has not spoken yet.
                        warn!(
                            "EcoFlow device {} has not reported since startup; if this persists, check the serial number",
                            device.serial
                        );
                    }
                    device.stale_reported = true;
                } else if !stale && device.stale_reported {
                    info!("EcoFlow device {} is reporting again", device.serial);
                    device.stale_reported = false;
                }
            }
        }
    }

    /// Run the session forever, reconnecting with backoff.
    async fn session_loop(
        api: Arc<A>,
        transport: Arc<Mutex<T>>,
        config: Config,
        inner: SharedInner,
        client_uuid: String,
        user_id: Arc<Mutex<Option<String>>>,
        to_engine: FromIntegrationSender,
    ) {
        let mut backoff = Backoff::default();

        loop {
            match Self::run_session(
                &api,
                &transport,
                &config,
                &inner,
                &client_uuid,
                &user_id,
                &to_engine,
            )
            .await
            {
                Ok(()) => {
                    info!("EcoFlow session ended; reconnecting");
                    backoff.reset();
                }
                Err(e) => {
                    warn!("EcoFlow session failed: {e}");
                }
            }

            // Commands cannot be sent without a live session, and a stale
            // account id would let one be attempted against a dead connection.
            *user_id.lock().await = None;

            let delay = backoff.next_delay();
            debug!(
                "reconnecting to EcoFlow in {delay:?} (consecutive failures: {})",
                backoff.attempts()
            );
            tokio::time::sleep(delay).await;
        }
    }

    /// One session: authenticate, connect, subscribe, then pump messages until
    /// the connection ends.
    async fn run_session(
        api: &A,
        transport: &Arc<Mutex<T>>,
        config: &Config,
        inner: &SharedInner,
        client_uuid: &str,
        user_id: &Arc<Mutex<Option<String>>>,
        to_engine: &FromIntegrationSender,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Both calls run on every attempt: neither credential advertises its
        // expiry, so refreshing is cheaper than detecting staleness.
        let session = api.login(&config.email, &config.password).await?;
        let credentials = api.certification(&session.token).await?;
        let client_id = topics::client_id(client_uuid, &session.user_id);

        info!(
            "connecting to EcoFlow broker {}:{}",
            credentials.host, credentials.port
        );

        // The receiving half is held here rather than behind the transport
        // lock, so that a command can be published while this loop is parked
        // waiting for the next message.
        let mut stream = {
            let mut transport = transport.lock().await;
            transport.connect(&credentials, &client_id).await?
        };

        *user_id.lock().await = Some(session.user_id.clone());

        // Sessions are clean, so subscriptions never survive a reconnect.
        let serials: Vec<String> = {
            let guard = inner.lock().await;
            guard.serial_to_node.keys().cloned().collect()
        };

        {
            let mut transport = transport.lock().await;
            for serial in &serials {
                for topic in topics::all_for_device(&session.user_id, serial) {
                    transport.subscribe(&topic).await?;
                }
            }
        }

        // Nothing arrives until a device pushes, so ask for a snapshot rather
        // than waiting out the incremental cadence.
        for serial in &serials {
            let request = ConfigWrite {
                active_display_property_full_upload: Some(true),
                active_runtime_property_full_upload: Some(true),
                ..Default::default()
            };
            if let Err(e) =
                Self::publish_config_write(transport, &session.user_id, serial, &request).await
            {
                warn!("failed to request a snapshot from {serial}: {e}");
            }
        }

        while let Some(message) = stream.next().await {
            Self::handle_incoming(&message, inner, to_engine).await;
        }

        Ok(())
    }

    /// Frame and publish a config write.
    async fn publish_config_write(
        transport: &Arc<Mutex<T>>,
        user_id: &str,
        serial: &str,
        write: &ConfigWrite,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let pdata = write.encode();
        let seq = rand::rng().random_range(SEQ_MIN..=SEQ_MAX);
        let frame = wire::encode_config_write(&pdata, serial, seq);

        let mut transport = transport.lock().await;
        transport
            .publish(&topics::set(user_id, serial), &frame)
            .await?;
        Ok(())
    }

    /// Route one MQTT message.
    ///
    /// A failure here logs and drops the message. Firmware revisions add
    /// fields and occasionally new command ids, so a decoding failure is not a
    /// reason to tear down a working connection.
    async fn handle_incoming(
        message: &Message,
        inner: &SharedInner,
        to_engine: &FromIntegrationSender,
    ) {
        let node_id = {
            let guard = inner.lock().await;
            // A serial always occupies a whole path segment, in both
            // `/app/device/property/{sn}` and `/app/{user}/{sn}/thing/...`.
            // Matching segments rather than substrings keeps one serial from
            // capturing another's traffic when one is a prefix of the other.
            match guard
                .serial_to_node
                .iter()
                .find(|(serial, _)| {
                    message
                        .topic
                        .split('/')
                        .any(|segment| segment == serial.as_str())
                })
                .map(|(_, node_id)| *node_id)
            {
                Some(node_id) => node_id,
                None => {
                    debug!("ignoring message on unrecognised topic {}", message.topic);
                    return;
                }
            }
        };

        let frame = match wire::decode_frame(&message.payload) {
            // Frames we do not recognise, and our own echoed commands, land
            // here. Both are normal.
            Ok(None) => return,
            Ok(Some(frame)) => frame,
            Err(e) => {
                warn!("dropping malformed EcoFlow frame on {}: {e}", message.topic);
                return;
            }
        };

        // Which field number carries a given reading is exactly what a
        // mismatch against real hardware turns on, and the decoders skip
        // anything unmapped without a word. Guarded because rendering the
        // census is not free.
        if tracing::enabled!(tracing::Level::TRACE) {
            match codec::field_census(&frame.pdata) {
                Ok(census) => tracing::trace!("{:?} fields: {census}", frame.payload),
                Err(e) => tracing::trace!("could not take a field census: {e}"),
            }
        }

        let now = std::time::Instant::now();
        let changed = {
            let mut guard = inner.lock().await;
            let device = match guard.devices.get_mut(&node_id) {
                Some(device) => device,
                None => return,
            };

            match frame.payload {
                wire::Payload::DisplayPropertyUpload => match codec::decode_display(&frame.pdata) {
                    Ok(delta) => device.state.apply_display(delta, now),
                    Err(e) => {
                        warn!("dropping malformed display upload: {e}");
                        return;
                    }
                },
                wire::Payload::RuntimePropertyUpload => match codec::decode_runtime(&frame.pdata) {
                    Ok(delta) => device.state.apply_runtime(delta, now),
                    Err(e) => {
                        warn!("dropping malformed runtime upload: {e}");
                        return;
                    }
                },
                wire::Payload::ConfigWriteAck => {
                    // An ack confirms receipt, not effect; the authoritative
                    // state is the next property upload.
                    match codec::decode_config_write_ack(&frame.pdata) {
                        Ok(ack) if ack.config_ok == Some(false) => {
                            warn!("device {} rejected a config write", device.serial);
                        }
                        Ok(_) => debug!("device {} acknowledged a config write", device.serial),
                        Err(e) => warn!("dropping malformed config write ack: {e}"),
                    }
                    return;
                }
            }

            Self::take_changed_clusters(device)
        };

        for (endpoint_id, cluster) in changed {
            Self::send_attribute_changed(node_id, endpoint_id, cluster, to_engine).await;
        }
    }

    /// Rebuild the device's endpoints and return the clusters that differ from
    /// what was last reported, updating the record of what has been published.
    ///
    /// Rebuilding wholesale and diffing keeps the merge logic free of
    /// change-tracking bookkeeping. The endpoint map is small and the device
    /// reports every few seconds, so the cost is irrelevant.
    fn take_changed_clusters(device: &mut Device) -> Vec<(EndpointId, Cluster)> {
        let rebuilt = wave3_matter::build_endpoints(&device.state);
        let mut changed = Vec::new();

        for (endpoint_id, endpoint) in &rebuilt {
            for (name, cluster) in &endpoint.clusters {
                let previous = device
                    .published
                    .get(endpoint_id)
                    .and_then(|e| e.clusters.get(name));
                if previous != Some(cluster) {
                    changed.push((*endpoint_id, cluster.clone()));
                }
            }
        }

        device.published = rebuilt;
        changed
    }

    async fn send_attribute_changed(
        node_id: NodeId,
        endpoint_id: EndpointId,
        cluster: Cluster,
        to_engine: &FromIntegrationSender,
    ) {
        if let Err(e) = to_engine
            .send(FromIntegrationMessage::AttributeChanged {
                node_id,
                endpoint_id,
                cluster,
            })
            .await
        {
            warn!("failed to send AttributeChanged: {e}");
        }
    }

    /// Translate a cluster command and publish it.
    async fn invoke_command(
        &self,
        node_id: NodeId,
        endpoint_id: EndpointId,
        command: ClusterCommand,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (serial, write) = {
            let guard = self.inner.lock().await;
            let device = guard
                .devices
                .get(&node_id)
                .ok_or_else(|| -> Box<dyn Error + Send> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("unknown node: {node_id}"),
                    ))
                })?;

            let write = wave3_matter::command_to_config_write(&device.state, endpoint_id, &command)
                .map_err(|e| -> Box<dyn Error + Send> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        e.to_string(),
                    ))
                })?;

            (device.serial.clone(), write)
        };

        if write.is_empty() {
            return Ok(());
        }

        // The command topic is user-scoped, so a command can only be sent
        // while a session is up.
        let user_id = match self.current_user_id().await {
            Some(user_id) => user_id,
            None => {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "not connected to EcoFlow",
                )));
            }
        };

        Self::publish_config_write(&self.transport, &user_id, &serial, &write)
            .await
            .map_err(|e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::other(e.to_string()))
            })?;

        info!("sent EcoFlow command to node {node_id} endpoint {endpoint_id}: {command:?}");

        // Apply the commanded values immediately so readers do not lag a full
        // upload period. The next report overwrites them; if none ever
        // confirms or contradicts them, the command was probably lost.
        let changed = {
            let mut guard = self.inner.lock().await;
            match guard.devices.get_mut(&node_id) {
                Some(device) => {
                    device.state.apply_optimistic(&write);
                    Self::take_changed_clusters(device)
                }
                None => Vec::new(),
            }
        };

        if let Some(to_engine) = &self.to_engine {
            for (endpoint_id, cluster) in changed {
                Self::send_attribute_changed(node_id, endpoint_id, cluster, to_engine).await;
            }
        }

        Ok(())
    }

    async fn current_user_id(&self) -> Option<String> {
        self.user_id.lock().await.clone()
    }
}

#[async_trait]
impl<A: EcoFlowApi + 'static, T: Transport + 'static> Integration for EcoFlowIntegration<A, T> {
    fn name(&self) -> &str {
        INTEGRATION_NAME
    }

    async fn setup(
        &mut self,
        tx: FromIntegrationSender,
        node_ids: NodeIdAllocator,
    ) -> Result<(), Box<dyn Error + Send>> {
        self.to_engine = Some(tx.clone());

        // Devices are declared, not discovered, so every node is known now.
        // Announcing them before any telemetry means the engine sees a stable
        // shape whose attributes fill in later.
        let nodes: Vec<(NodeId, Node)> = {
            let mut guard = self.inner.lock().await;

            for declared in self.declared.drain(..) {
                let node_id = node_ids.allocate();
                let state = DeviceState::default();

                guard
                    .serial_to_node
                    .insert(declared.serial.clone(), node_id);
                guard.devices.insert(
                    node_id,
                    Device {
                        node_id,
                        entity_id: declared.entity_id,
                        name: declared.name,
                        serial: declared.serial,
                        published: wave3_matter::build_endpoints(&state),
                        state,
                        stale_reported: false,
                    },
                );
            }

            guard
                .devices
                .values()
                .map(|device| (device.node_id, device.node()))
                .collect()
        };

        for (node_id, node) in nodes {
            info!(
                "declared EcoFlow device: {} (node {node_id})",
                node.entity_id
            );
            if let Err(e) = tx
                .send(FromIntegrationMessage::NodeAdded { node_id, node })
                .await
            {
                warn!("failed to send NodeAdded: {e}");
            }
        }

        let api = self.api.clone();
        let transport = self.transport.clone();
        let config = self.config.clone();
        let inner = self.inner.clone();
        let client_uuid = self.client_uuid.clone();
        let user_id = self.user_id.clone();

        let watchdog_inner = self.inner.clone();
        self.watchdog_task = Some(tokio::spawn(async move {
            Self::staleness_watchdog(watchdog_inner).await;
        }));

        self.session_task = Some(tokio::spawn(async move {
            Self::session_loop(api, transport, config, inner, client_uuid, user_id, tx).await;
        }));

        Ok(())
    }

    async fn handle_message(
        &mut self,
        msg: ToIntegrationMessage,
    ) -> Result<(), Box<dyn Error + Send>> {
        match msg {
            ToIntegrationMessage::InvokeCommand {
                node_id,
                endpoint_id,
                command,
            } => self.invoke_command(node_id, endpoint_id, command).await,
        }
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
        if let Some(task) = self.session_task.take() {
            task.abort();
        }
        if let Some(task) = self.watchdog_task.take() {
            task.abort();
        }
        info!("EcoFlow integration shutting down");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::mpsc;

    use super::*;
    use crate::integrations::ecoflow::cloud::auth::AuthError;
    use crate::integrations::ecoflow::cloud::auth::MqttCredentials;
    use crate::integrations::ecoflow::cloud::auth::Session;
    use crate::integrations::ecoflow::cloud::transport::MessageStream;
    use crate::integrations::ecoflow::cloud::transport::TransportError;
    use crate::integrations::ecoflow::config::DeviceConfig;
    use crate::integrations::ecoflow::protobuf::Writer;
    use crate::integrations::ecoflow::wave3::fields::display;
    use crate::integrations::ecoflow::wave3::wire::CMD_ID_DISPLAY_FULL;
    use crate::integrations::ecoflow::wave3::wire::encode_inbound_for_test;
    use crate::matter::OnOffCommand;

    const SERIAL: &str = "AB123";
    const USER_ID: &str = "1234567890";

    struct MockApi;

    #[async_trait]
    impl EcoFlowApi for MockApi {
        async fn login(&self, _email: &str, _password: &str) -> Result<Session, AuthError> {
            Ok(Session {
                token: "token".to_string(),
                user_id: USER_ID.to_string(),
            })
        }

        async fn certification(&self, _token: &str) -> Result<MqttCredentials, AuthError> {
            Ok(MqttCredentials {
                host: "broker.invalid".to_string(),
                port: 8883,
                username: "u".to_string(),
                password: "p".to_string(),
            })
        }
    }

    #[derive(Default)]
    struct MockState {
        published: Vec<(String, Vec<u8>)>,
        subscribed: Vec<String>,
    }

    struct MockTransport {
        state: Arc<Mutex<MockState>>,
        /// Handed out by the first `connect`; later connects get a stream that
        /// ends at once, so a test does not have to model reconnection.
        stream: Option<mpsc::UnboundedReceiver<Message>>,
    }

    #[async_trait]
    impl Transport for MockTransport {
        async fn connect(
            &mut self,
            _credentials: &MqttCredentials,
            _client_id: &str,
        ) -> Result<MessageStream, TransportError> {
            match self.stream.take() {
                Some(rx) => Ok(MessageStream::new(rx)),
                None => {
                    let (_tx, rx) = mpsc::unbounded_channel();
                    Ok(MessageStream::new(rx))
                }
            }
        }

        async fn subscribe(&mut self, topic: &str) -> Result<(), TransportError> {
            self.state.lock().await.subscribed.push(topic.to_string());
            Ok(())
        }

        async fn publish(&mut self, topic: &str, payload: &[u8]) -> Result<(), TransportError> {
            self.state
                .lock()
                .await
                .published
                .push((topic.to_string(), payload.to_vec()));
            Ok(())
        }
    }

    fn config() -> Config {
        let mut devices = HashMap::new();
        devices.insert(
            "bedroom".to_string(),
            DeviceConfig {
                serial: SERIAL.to_string(),
                name: Some("Bedroom AC".to_string()),
            },
        );
        Config {
            api_host: "api.invalid".to_string(),
            email: "user@example.com".to_string(),
            password: "hunter2".to_string(),
            devices,
        }
    }

    /// Build the integration plus handles onto the mock's inbound channel and
    /// recorded traffic.
    #[allow(clippy::type_complexity)]
    fn harness() -> (
        EcoFlowIntegration<MockApi, MockTransport>,
        mpsc::UnboundedSender<Message>,
        Arc<Mutex<MockState>>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(MockState::default()));
        let transport = MockTransport {
            state: state.clone(),
            stream: Some(rx),
        };
        let integration = EcoFlowIntegration::new(MockApi, transport, &config());
        (integration, tx, state)
    }

    /// A display upload reporting an ambient temperature and cool mode.
    fn telemetry_frame(celsius: f32) -> Vec<u8> {
        let mut payload = Writer::new();
        payload.write_f32(display::TEMP_AMBIENT, celsius);
        payload.write_u32(display::WAVE_OPERATING_MODE, 1);
        payload.write_u32(display::DEV_SLEEP_STATE, 0);

        encode_inbound_for_test(CMD_ID_DISPLAY_FULL, &payload.into_vec(), 500, 66, 1, SERIAL)
    }

    /// Await a message from the engine channel, failing rather than hanging.
    async fn next_engine_message(
        rx: &mut mpsc::Receiver<FromIntegrationMessage>,
    ) -> FromIntegrationMessage {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for a message from the integration")
            .expect("integration channel closed")
    }

    #[tokio::test]
    async fn declared_devices_are_announced_before_any_telemetry() {
        let (mut integration, _inbound, _state) = harness();
        let (tx, mut rx) = mpsc::channel(64);

        integration
            .setup(tx, NodeIdAllocator::for_test())
            .await
            .unwrap();

        match next_engine_message(&mut rx).await {
            FromIntegrationMessage::NodeAdded { node_id, node } => {
                assert_eq!(node_id, NodeId::from_raw(1));
                assert_eq!(node.entity_id, "climate.bedroom");
                assert_eq!(node.name.as_deref(), Some("Bedroom AC"));
                assert_eq!(node.integration, INTEGRATION_NAME);
                // The full shape exists immediately; attributes are null.
                assert!(node.endpoints.contains_key(&wave3_matter::EP_BATTERY));
                assert!(node.endpoints.contains_key(&wave3_matter::EP_POWER_PV));
            }
            other => panic!("expected NodeAdded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_session_subscribes_every_topic_and_asks_for_a_snapshot() {
        let (mut integration, _inbound, state) = harness();
        let (tx, mut rx) = mpsc::channel(64);

        integration
            .setup(tx, NodeIdAllocator::for_test())
            .await
            .unwrap();
        next_engine_message(&mut rx).await;

        // Let the session task authenticate, connect and subscribe.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let state = state.lock().await;
        assert_eq!(state.subscribed.len(), 5, "{:?}", state.subscribed);
        assert!(
            state
                .subscribed
                .contains(&format!("/app/device/property/{SERIAL}"))
        );
        assert!(
            state
                .subscribed
                .contains(&format!("/app/{USER_ID}/{SERIAL}/thing/property/set"))
        );

        // Protobuf-only firmware ignores the JSON latestQuotas request, so the
        // snapshot is asked for with a config write on the set topic.
        assert_eq!(state.published.len(), 1);
        assert_eq!(
            state.published[0].0,
            format!("/app/{USER_ID}/{SERIAL}/thing/property/set")
        );
    }

    #[tokio::test]
    async fn telemetry_reaches_the_engine_as_attribute_changes() {
        let (mut integration, inbound, _state) = harness();
        let (tx, mut rx) = mpsc::channel(64);

        integration
            .setup(tx, NodeIdAllocator::for_test())
            .await
            .unwrap();
        next_engine_message(&mut rx).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        inbound
            .send(Message {
                topic: format!("/app/device/property/{SERIAL}"),
                payload: telemetry_frame(21.5),
            })
            .unwrap();

        let mut saw_temperature = false;
        for _ in 0..12 {
            match next_engine_message(&mut rx).await {
                FromIntegrationMessage::AttributeChanged { cluster, .. } => {
                    if let Cluster::TemperatureMeasurement(t) = cluster {
                        if t.measured_value == Some(2150) {
                            saw_temperature = true;
                            break;
                        }
                    }
                }
                other => panic!("expected AttributeChanged, got {other:?}"),
            }
        }
        assert!(saw_temperature, "no temperature reading reached the engine");
    }

    #[tokio::test]
    async fn an_unrecognised_topic_is_ignored() {
        let (mut integration, inbound, _state) = harness();
        let (tx, mut rx) = mpsc::channel(64);

        integration
            .setup(tx, NodeIdAllocator::for_test())
            .await
            .unwrap();
        next_engine_message(&mut rx).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        inbound
            .send(Message {
                topic: "/app/device/property/SOMEONE-ELSE".to_string(),
                payload: telemetry_frame(21.5),
            })
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            rx.try_recv().is_err(),
            "a message for an undeclared device should produce nothing"
        );
    }

    #[tokio::test]
    async fn our_own_echoed_commands_are_ignored() {
        let (mut integration, inbound, _state) = harness();
        let (tx, mut rx) = mpsc::channel(64);

        integration
            .setup(tx, NodeIdAllocator::for_test())
            .await
            .unwrap();
        next_engine_message(&mut rx).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // The broker echoes what we publish on the set topic back to us.
        let write = ConfigWrite {
            cfg_temp_set: Some(22.0),
            ..Default::default()
        };
        inbound
            .send(Message {
                topic: format!("/app/{USER_ID}/{SERIAL}/thing/property/set"),
                payload: wire::encode_config_write(&write.encode(), SERIAL, 500),
            })
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            rx.try_recv().is_err(),
            "an echoed command should not be mistaken for telemetry"
        );
    }

    #[tokio::test]
    async fn a_command_is_published_while_the_receive_loop_is_waiting() {
        // The receive loop parks on the message stream for as long as the
        // device stays quiet. If publishing needed the same lock, no command
        // would ever go out.
        let (mut integration, _inbound, state) = harness();
        let (tx, mut rx) = mpsc::channel(64);

        integration
            .setup(tx, NodeIdAllocator::for_test())
            .await
            .unwrap();
        next_engine_message(&mut rx).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        let before = state.lock().await.published.len();

        tokio::time::timeout(
            Duration::from_secs(5),
            integration.handle_message(ToIntegrationMessage::InvokeCommand {
                node_id: NodeId::from_raw(1),
                endpoint_id: wave3_matter::EP_AIR_CONDITIONER,
                command: ClusterCommand::OnOff(OnOffCommand::Off),
            }),
        )
        .await
        .expect("publishing a command blocked on the receive loop")
        .expect("command should be accepted");

        let state = state.lock().await;
        assert_eq!(state.published.len(), before + 1);

        let (topic, payload) = state.published.last().unwrap();
        assert_eq!(
            topic,
            &format!("/app/{USER_ID}/{SERIAL}/thing/property/set")
        );
        let expected = ConfigWrite {
            cfg_sys_pause: Some(true),
            ..Default::default()
        }
        .encode();
        assert!(
            payload.windows(expected.len()).any(|w| w == expected),
            "published frame does not carry cfg_sys_pause"
        );
    }

    #[tokio::test]
    async fn an_optimistic_update_is_reported_without_waiting_for_the_device() {
        let (mut integration, _inbound, _state) = harness();
        let (tx, mut rx) = mpsc::channel(64);

        integration
            .setup(tx, NodeIdAllocator::for_test())
            .await
            .unwrap();
        next_engine_message(&mut rx).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        integration
            .handle_message(ToIntegrationMessage::InvokeCommand {
                node_id: NodeId::from_raw(1),
                endpoint_id: wave3_matter::EP_BEEPER,
                command: ClusterCommand::OnOff(OnOffCommand::On),
            })
            .await
            .unwrap();

        match next_engine_message(&mut rx).await {
            FromIntegrationMessage::AttributeChanged {
                endpoint_id,
                cluster,
                ..
            } => {
                assert_eq!(endpoint_id, wave3_matter::EP_BEEPER);
                assert!(matches!(cluster, Cluster::OnOff(c) if c.on_off));
            }
            other => panic!("expected AttributeChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_command_for_an_unknown_node_is_refused() {
        let (mut integration, _inbound, _state) = harness();
        let (tx, mut rx) = mpsc::channel(64);

        integration
            .setup(tx, NodeIdAllocator::for_test())
            .await
            .unwrap();
        next_engine_message(&mut rx).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        let result = integration
            .handle_message(ToIntegrationMessage::InvokeCommand {
                node_id: NodeId::from_raw(99),
                endpoint_id: wave3_matter::EP_AIR_CONDITIONER,
                command: ClusterCommand::OnOff(OnOffCommand::On),
            })
            .await;

        assert!(result.is_err());
    }
}
