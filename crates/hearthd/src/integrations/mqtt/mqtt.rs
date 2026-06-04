use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::MqttConfig;
use super::binary_sensor::BinarySensor;
use super::client::MqttClient;
use super::client::MqttMessage;
use super::discovery::DiscoveryMessage;
use super::discovery::parse_discovery_topic;
use super::light::Light;
use super::light::Z2M_ENDPOINT;
use crate::engine::FromIntegrationMessage;
use crate::engine::FromIntegrationSender;
use crate::engine::Integration;
use crate::engine::ToIntegrationMessage;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;
use crate::matter::NodeId;

/// Integration name reported to the engine.
const INTEGRATION_NAME: &str = "mqtt";

/// MQTT-side entity. The integration owns one of these per discovered node;
/// the engine sees only `Node`s built from these.
enum MqttEntity {
    Light(Arc<Mutex<Light>>),
    BinarySensor(Arc<Mutex<BinarySensor>>),
}

/// Shared inner state for the integration. All maps are keyed by NodeId.
#[derive(Default)]
struct Inner {
    entities: HashMap<NodeId, MqttEntity>,
    /// Reverse index: state-update topic → NodeId
    topic_to_node: HashMap<String, NodeId>,
    /// Reverse index: entity_id alias → NodeId (for re-discovery / removal)
    entity_to_node: HashMap<String, NodeId>,
}

type SharedInner = Arc<Mutex<Inner>>;

/// MQTT Integration for hearthd.
///
/// Translates between Zigbee2MQTT and the Matter-shaped engine API. All
/// state crossing the engine boundary uses `crate::matter` types.
pub struct MqttIntegration<C: MqttClient> {
    client: Arc<Mutex<C>>,
    config: MqttConfig,
    inner: SharedInner,
    next_node_id: Arc<AtomicU64>,
    to_engine: Option<FromIntegrationSender>,
    /// Handle to the background message processing task
    _message_task: Option<JoinHandle<()>>,
}

impl<C: MqttClient> MqttIntegration<C> {
    /// Create a new MQTT integration
    pub fn new(client: C, config: &MqttConfig) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            config: config.clone(),
            inner: Arc::new(Mutex::new(Inner::default())),
            next_node_id: Arc::new(AtomicU64::new(1)),
            to_engine: None,
            _message_task: None,
        }
    }

    /// Process incoming MQTT messages in a background task.
    async fn process_messages_task(
        client: Arc<Mutex<C>>,
        config: MqttConfig,
        inner: SharedInner,
        next_node_id: Arc<AtomicU64>,
        to_engine: FromIntegrationSender,
    ) {
        loop {
            let msg = {
                let mut client_guard = client.lock().await;
                tokio::time::timeout(
                    std::time::Duration::from_millis(100),
                    client_guard.poll_message(),
                )
                .await
                .unwrap_or_default()
            };

            match msg {
                Some(msg) => {
                    info!("Received message on topic: {}", msg.topic);

                    if msg.topic.ends_with("/config") {
                        if let Err(e) = Self::handle_discovery(
                            &msg,
                            &config,
                            &client,
                            &inner,
                            &next_node_id,
                            &to_engine,
                        )
                        .await
                        {
                            warn!("Error handling discovery message: {}", e);
                        }
                    } else if let Err(e) = Self::handle_state_update(&msg, &inner, &to_engine).await
                    {
                        warn!("Error handling state update: {}", e);
                    }
                }
                None => {
                    tokio::task::yield_now().await;
                }
            }
        }
    }

    async fn handle_discovery(
        msg: &MqttMessage,
        config: &MqttConfig,
        client: &Arc<Mutex<C>>,
        inner: &SharedInner,
        next_node_id: &AtomicU64,
        to_engine: &FromIntegrationSender,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (component, node_id_str, object_id) =
            parse_discovery_topic(&msg.topic, &config.discovery_prefix).ok_or_else(
                || -> Box<dyn Error + Send> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Failed to parse discovery topic",
                    ))
                },
            )?;

        debug!(
            "Discovery: component={}, node_id={}, object_id={}",
            component, node_id_str, object_id
        );

        match component.as_str() {
            "light" => {
                Self::handle_light_discovery(
                    msg,
                    client,
                    inner,
                    next_node_id,
                    to_engine,
                    &node_id_str,
                )
                .await
            }
            "binary_sensor" => {
                // TODO: Z2M also publishes auxiliary `sensor` components
                // (battery, linkquality, illuminance) that should become
                // their own Matter clusters.
                Self::handle_binary_sensor_discovery(
                    msg,
                    client,
                    inner,
                    next_node_id,
                    to_engine,
                    &node_id_str,
                )
                .await
            }
            _ => {
                debug!("Ignoring unsupported component: {}", component);
                Ok(())
            }
        }
    }

    async fn handle_light_discovery(
        msg: &MqttMessage,
        client: &Arc<Mutex<C>>,
        inner: &SharedInner,
        next_node_id: &AtomicU64,
        to_engine: &FromIntegrationSender,
        z2m_node_id: &str,
    ) -> Result<(), Box<dyn Error + Send>> {
        let entity_id = format!("light.{}", z2m_node_id);

        // Empty payload = retained discovery deletion
        if msg.payload.is_empty() {
            Self::remove_entity_by_alias(&entity_id, inner, to_engine).await;
            return Ok(());
        }

        // Already-known entity: ignore (Z2M can re-publish discovery)
        {
            let guard = inner.lock().await;
            if guard.entity_to_node.contains_key(&entity_id) {
                debug!("Ignoring re-discovery for {}", entity_id);
                return Ok(());
            }
        }

        let discovery: DiscoveryMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })?;

        let light = Light::from_discovery(discovery, entity_id.clone(), z2m_node_id.to_string())
            .map_err(|e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ))
            })?;

        let state_topic = light.state_topic.clone();
        let node = light.to_node(INTEGRATION_NAME);
        info!("Discovered light entity: {} ({})", light.name, entity_id);

        let node_id = next_node_id.fetch_add(1, Ordering::Relaxed);
        let light_arc = Arc::new(Mutex::new(light));

        {
            let mut guard = inner.lock().await;
            guard.entities.insert(node_id, MqttEntity::Light(light_arc));
            guard.topic_to_node.insert(state_topic.clone(), node_id);
            guard.entity_to_node.insert(entity_id, node_id);
        }

        // Subscribe after registering so the retained state message routes correctly.
        {
            let mut client_guard = client.lock().await;
            client_guard.subscribe(&state_topic).await?;
        }

        Self::send_node_added(node_id, node, to_engine).await;

        Ok(())
    }

    async fn handle_binary_sensor_discovery(
        msg: &MqttMessage,
        client: &Arc<Mutex<C>>,
        inner: &SharedInner,
        next_node_id: &AtomicU64,
        to_engine: &FromIntegrationSender,
        z2m_node_id: &str,
    ) -> Result<(), Box<dyn Error + Send>> {
        let entity_id = format!("binary_sensor.{}", z2m_node_id);

        if msg.payload.is_empty() {
            Self::remove_entity_by_alias(&entity_id, inner, to_engine).await;
            return Ok(());
        }

        {
            let guard = inner.lock().await;
            if guard.entity_to_node.contains_key(&entity_id) {
                debug!("Ignoring re-discovery for {}", entity_id);
                return Ok(());
            }
        }

        let discovery: DiscoveryMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })?;

        // Only motion-style sensors map to Matter's OccupancySensing cluster.
        // Z2M reports many other binary-sensor device classes (door, vibration,
        // battery, ...) on the same discovery topic; skip those until we model
        // their clusters.
        match discovery.device_class.as_deref() {
            Some("motion") | Some("occupancy") | Some("presence") => {}
            other => {
                warn!(
                    "Skipping binary sensor {} with unsupported device_class {:?}",
                    entity_id, other
                );
                return Ok(());
            }
        }

        let sensor =
            BinarySensor::from_discovery(discovery, entity_id.clone(), z2m_node_id.to_string())
                .map_err(|e| -> Box<dyn Error + Send> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    ))
                })?;

        let state_topic = sensor.state_topic.clone();
        let node = sensor.to_node(INTEGRATION_NAME);
        info!(
            "Discovered binary sensor entity: {} ({})",
            sensor.name, entity_id
        );

        let node_id = next_node_id.fetch_add(1, Ordering::Relaxed);
        let sensor_arc = Arc::new(Mutex::new(sensor));

        {
            let mut guard = inner.lock().await;
            guard
                .entities
                .insert(node_id, MqttEntity::BinarySensor(sensor_arc));
            guard.topic_to_node.insert(state_topic.clone(), node_id);
            guard.entity_to_node.insert(entity_id, node_id);
        }

        {
            let mut client_guard = client.lock().await;
            client_guard.subscribe(&state_topic).await?;
        }

        Self::send_node_added(node_id, node, to_engine).await;

        Ok(())
    }

    /// Remove an entity given its entity_id alias and notify the engine.
    async fn remove_entity_by_alias(
        entity_id: &str,
        inner: &SharedInner,
        to_engine: &FromIntegrationSender,
    ) {
        let removed = {
            let mut guard = inner.lock().await;
            if let Some(&node_id) = guard.entity_to_node.get(entity_id) {
                guard.entity_to_node.remove(entity_id);
                guard.entities.remove(&node_id);
                guard.topic_to_node.retain(|_, &mut v| v != node_id);
                Some(node_id)
            } else {
                None
            }
        };
        if let Some(node_id) = removed {
            info!("Removed entity: {} (node {})", entity_id, node_id);
            if let Err(e) = to_engine
                .send(FromIntegrationMessage::NodeRemoved { node_id })
                .await
            {
                warn!("Failed to send NodeRemoved: {}", e);
            }
        }
    }

    async fn send_node_added(
        node_id: NodeId,
        node: crate::matter::Node,
        to_engine: &FromIntegrationSender,
    ) {
        if let Err(e) = to_engine
            .send(FromIntegrationMessage::NodeAdded { node_id, node })
            .await
        {
            warn!("Failed to send NodeAdded message: {}", e);
        }
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
            warn!("Failed to send AttributeChanged message: {}", e);
        }
    }

    async fn handle_state_update(
        msg: &MqttMessage,
        inner: &SharedInner,
        to_engine: &FromIntegrationSender,
    ) -> Result<(), Box<dyn Error + Send>> {
        // Resolve topic → (NodeId, entity handle) and release the outer lock
        // before parsing the payload.
        let (node_id, entity) = {
            let guard = inner.lock().await;
            let node_id = match guard.topic_to_node.get(&msg.topic) {
                Some(id) => *id,
                None => return Ok(()),
            };
            let entity = match guard.entities.get(&node_id) {
                Some(MqttEntity::Light(l)) => MqttEntity::Light(l.clone()),
                Some(MqttEntity::BinarySensor(b)) => MqttEntity::BinarySensor(b.clone()),
                None => return Ok(()),
            };
            (node_id, entity)
        };

        match entity {
            MqttEntity::Light(light_arc) => {
                let clusters = {
                    let mut light = light_arc.lock().await;
                    light.apply_state_payload(&msg.payload).map_err(
                        |e| -> Box<dyn Error + Send> {
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                e.to_string(),
                            ))
                        },
                    )?
                };
                for cluster in clusters {
                    Self::send_attribute_changed(node_id, Z2M_ENDPOINT, cluster, to_engine).await;
                }
            }
            MqttEntity::BinarySensor(sensor_arc) => {
                let cluster = {
                    let mut sensor = sensor_arc.lock().await;
                    sensor.apply_state_payload(&msg.payload).map_err(
                        |e| -> Box<dyn Error + Send> {
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                e.to_string(),
                            ))
                        },
                    )?
                };
                if let Some(cluster) = cluster {
                    Self::send_attribute_changed(node_id, Z2M_ENDPOINT, cluster, to_engine).await;
                }
            }
        }

        Ok(())
    }

    /// Execute a cluster command against a discovered node.
    async fn invoke_command(
        &self,
        node_id: NodeId,
        endpoint_id: EndpointId,
        command: ClusterCommand,
    ) -> Result<(), Box<dyn Error + Send>> {
        if endpoint_id != Z2M_ENDPOINT {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unknown endpoint {} on node {}", endpoint_id, node_id),
            )));
        }

        let light_arc = {
            let guard = self.inner.lock().await;
            match guard.entities.get(&node_id) {
                Some(MqttEntity::Light(l)) => l.clone(),
                Some(MqttEntity::BinarySensor(_)) => {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("Node {} is a read-only sensor", node_id),
                    )));
                }
                None => {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Unknown node: {}", node_id),
                    )));
                }
            }
        };

        let (payload, command_topic) = {
            let light = light_arc.lock().await;
            let payload =
                light
                    .command_payload(&command)
                    .map_err(|e| -> Box<dyn Error + Send> {
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            e.to_string(),
                        ))
                    })?;
            (payload, light.command_topic.clone())
        };

        {
            let mut client = self.client.lock().await;
            client.publish(&command_topic, &payload, false).await?;
        }

        info!(
            "Sent command to node {} (endpoint {}): {:?}",
            node_id, endpoint_id, command
        );

        Ok(())
    }
}

#[async_trait]
impl<C: MqttClient + 'static> Integration for MqttIntegration<C> {
    fn name(&self) -> &str {
        INTEGRATION_NAME
    }

    async fn setup(&mut self, tx: FromIntegrationSender) -> Result<(), Box<dyn Error + Send>> {
        self.to_engine = Some(tx.clone());

        info!(
            "Connecting to MQTT broker at {}:{}",
            self.config.broker, self.config.port
        );
        {
            let mut client = self.client.lock().await;
            client.connect().await?;
        }
        info!("Connected to MQTT broker");

        let light_discovery = format!("{}/light/+/+/config", self.config.discovery_prefix);
        let binary_sensor_discovery =
            format!("{}/binary_sensor/+/+/config", self.config.discovery_prefix);
        info!(
            "Subscribing to discovery topics: {}, {}",
            light_discovery, binary_sensor_discovery
        );
        {
            let mut client = self.client.lock().await;
            client.subscribe(&light_discovery).await?;
            client.subscribe(&binary_sensor_discovery).await?;
        }

        info!("MQTT integration setup complete, spawning message processing task...");

        let client = self.client.clone();
        let config = self.config.clone();
        let inner = self.inner.clone();
        let next_node_id = self.next_node_id.clone();

        let task = tokio::spawn(async move {
            Self::process_messages_task(client, config, inner, next_node_id, tx).await;
        });
        self._message_task = Some(task);

        info!("MQTT integration ready to handle commands");
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
            } => {
                info!(
                    "Handling InvokeCommand for node {} endpoint {}: {:?}",
                    node_id, endpoint_id, command
                );
                self.invoke_command(node_id, endpoint_id, command).await?;
            }
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
        info!("MQTT integration shutting down");
        Ok(())
    }
}

// TryFrom implementation for config conversion
impl TryFrom<&crate::config::IntegrationsConfig>
    for MqttIntegration<crate::integrations::mqtt::client::RumqttcClient>
{
    type Error = Box<dyn Error>;

    fn try_from(cfg: &crate::config::IntegrationsConfig) -> Result<Self, Self::Error> {
        let mqtt_config = cfg
            .mqtt
            .as_ref()
            .ok_or("MQTT configuration is missing")?
            .clone();

        let client = crate::integrations::mqtt::client::RumqttcClient::new(&mqtt_config)?;
        Ok(MqttIntegration::new(client, &mqtt_config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::mqtt::client::MockMqttClient;

    #[tokio::test]
    async fn integration_starts_empty() {
        let client = MockMqttClient::new();
        let config = MqttConfig {
            broker: "localhost".to_string(),
            port: 1883,
            client_id: "test".to_string(),
            discovery_prefix: "homeassistant".to_string(),
            username: None,
            password: None,
        };
        let integration = MqttIntegration::new(client, &config);

        let guard = integration.inner.lock().await;
        assert!(guard.entities.is_empty());
        assert!(guard.topic_to_node.is_empty());
        assert!(guard.entity_to_node.is_empty());
    }
}
