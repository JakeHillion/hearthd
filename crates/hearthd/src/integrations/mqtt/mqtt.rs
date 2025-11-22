use crate::engine::{Integration, MessageSender};
use crate::integrations::mqtt::{
    client::{MqttClient, MqttMessage},
    config::MqttConfig,
    discovery::{DiscoveryMessage, parse_discovery_topic},
    light::{Light, LightState},
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Demo state for light control demonstration
#[derive(Debug, Clone, PartialEq)]
enum DemoState {
    NotStarted,
    WaitingToTurnOn,
    WaitingToTurnOff,
    Complete,
}

/// MQTT Integration for hearthd
///
/// Handles MQTT communication with Zigbee2MQTT and other MQTT-based devices.
/// Currently supports Light entities as MVP.
pub struct MqttIntegration<C: MqttClient> {
    client: C,
    config: MqttConfig,
    lights: HashMap<String, Arc<Mutex<Light>>>,
    demo_state: DemoState,
    demo_target_light: Option<String>,
    to_engine: Option<MessageSender>,
}

impl<C: MqttClient> MqttIntegration<C> {
    /// Create a new MQTT integration
    pub fn new(client: C, config: MqttConfig) -> Self {
        Self {
            client,
            config,
            lights: HashMap::new(),
            demo_state: DemoState::NotStarted,
            demo_target_light: None,
            to_engine: None,
        }
    }

    /// Process incoming MQTT messages
    async fn process_messages(&mut self) -> Result<(), Box<dyn Error + Send>> {
        use tokio::time::{Duration, interval};

        let mut demo_interval = interval(Duration::from_secs(10));

        loop {
            tokio::select! {
                // Handle incoming MQTT messages
                msg_opt = self.client.poll_message() => {
                    if let Some(msg) = msg_opt {
                        info!("Received message on topic: {}", msg.topic);

                        if msg.topic.ends_with("/config") {
                            if let Err(e) = self.handle_discovery(msg).await {
                                warn!("Error handling discovery message: {}", e);
                            }
                        } else if let Err(e) = self.handle_state_update(msg).await {
                            warn!("Error handling state update: {}", e);
                        }
                    } else {
                        warn!("poll_message returned None - this shouldn't happen with blocking recv");
                    }
                }

                // Demo: control Living Room desk lights
                _ = demo_interval.tick() => {
                    if self.demo_state == DemoState::NotStarted && !self.lights.is_empty() {
                        // Find the Living Room desk lights group
                        for (entity_id, light_arc) in &self.lights {
                            let light = light_arc.lock().await;
                            if light.name.contains("Living Room desk lights") ||
                               entity_id == "light.1221051039810110150109113116116_2" {
                                info!("Demo: Found target light '{}' ({})", light.name, entity_id);
                                self.demo_target_light = Some(entity_id.clone());
                                self.demo_state = DemoState::WaitingToTurnOn;
                                break;
                            }
                        }
                    } else if self.demo_state == DemoState::WaitingToTurnOn {
                        if let Some(light_id) = self.demo_target_light.clone() {
                            info!("Demo: Turning ON light '{}'", light_id);
                            let state = LightState {
                                on: true,
                                brightness: Some(254),
                            };
                            if let Err(e) = self.send_light_command(&light_id, state).await {
                                warn!("Demo: Failed to turn on light: {}", e);
                            }
                            self.demo_state = DemoState::WaitingToTurnOff;
                        }
                    } else if self.demo_state == DemoState::WaitingToTurnOff {
                        if let Some(light_id) = self.demo_target_light.clone() {
                            info!("Demo: Turning OFF light '{}'", light_id);
                            let state = LightState {
                                on: false,
                                brightness: None,
                            };
                            if let Err(e) = self.send_light_command(&light_id, state).await {
                                warn!("Demo: Failed to turn off light: {}", e);
                            }
                            self.demo_state = DemoState::Complete;
                        }
                    }
                }
            }
        }
    }

    /// Handle a discovery message from Zigbee2MQTT
    async fn handle_discovery(&mut self, msg: MqttMessage) -> Result<(), Box<dyn Error + Send>> {
        // Parse the discovery topic
        let (component, node_id, object_id) =
            parse_discovery_topic(&msg.topic, &self.config.discovery_prefix).ok_or_else(
                || -> Box<dyn Error + Send> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Failed to parse discovery topic",
                    ))
                },
            )?;

        debug!(
            "Discovery: component={}, node_id={}, object_id={}",
            component, node_id, object_id
        );

        // Only handle light components for MVP
        if component != "light" {
            debug!("Ignoring non-light component: {}", component);
            return Ok(());
        }

        // Parse discovery payload
        if msg.payload.is_empty() {
            // Empty payload means the entity should be removed
            let entity_id = format!("light.{}", node_id);
            if self.lights.remove(&entity_id).is_some() {
                info!("Removed light entity: {}", entity_id);
                // TODO: Notify engine of entity removal
            }
            return Ok(());
        }

        let discovery: DiscoveryMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })?;
        // Use node_id for unique entity_id since object_id is often just "light"
        let entity_id = format!("light.{}", node_id);

        // Create the light entity
        let light = Light::from_discovery(discovery, entity_id.clone(), node_id).map_err(
            |e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ))
            },
        )?;

        // Subscribe to state topic
        self.client.subscribe(&light.state_topic).await?;

        info!("Discovered light entity: {} ({})", light.name, entity_id);

        // Wrap in Arc<Mutex> for shared ownership with Engine
        let light_arc = Arc::new(Mutex::new(light));

        // Store the light
        self.lights.insert(entity_id.clone(), light_arc.clone());

        // Register entity with engine
        self.register_entity(&entity_id, light_arc);

        Ok(())
    }

    /// Handle a state update message
    async fn handle_state_update(&mut self, msg: MqttMessage) -> Result<(), Box<dyn Error + Send>> {
        // Find which light this state update is for
        let mut entity_to_update: Option<(String, LightState)> = None;

        for (entity_id, light_arc) in self.lights.iter() {
            let mut light = light_arc.lock().await;
            if msg.topic == light.state_topic {
                debug!("State update for light: {}", entity_id);
                light
                    .update_state(&msg.payload)
                    .map_err(|e| -> Box<dyn Error + Send> {
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            e.to_string(),
                        ))
                    })?;
                entity_to_update = Some((entity_id.clone(), light.state.clone()));
                break;
            }
        }

        // Report state change after releasing the lock
        if let Some((entity_id, state)) = entity_to_update {
            self.report_state_change(&entity_id, &state);
        }

        Ok(())
    }

    /// Send a command to a light
    pub async fn send_light_command(
        &mut self,
        light_id: &str,
        state: LightState,
    ) -> Result<(), Box<dyn Error + Send>> {
        let light_arc = self
            .lights
            .get(light_id)
            .ok_or_else(|| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Light not found: {}", light_id),
                ))
            })?
            .clone();

        let light = light_arc.lock().await;
        let payload = light
            .command_payload(&state)
            .map_err(|e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ))
            })?;

        let command_topic = light.command_topic.clone();
        drop(light); // Release lock before async call

        self.client.publish(&command_topic, &payload, false).await?;

        info!("Sent command to light {}: {:?}", light_id, state);

        Ok(())
    }

    /// Register an entity with the engine
    fn register_entity(&self, entity_id: &str, light: Arc<Mutex<Light>>) {
        if let Some(ref tx) = self.to_engine {
            let msg = crate::engine::Message::from_integration(
                crate::engine::MessagePayload::EntityDiscovered {
                    entity_id: entity_id.to_string(),
                    entity: light,
                },
            );
            if let Err(e) = tx.send(msg) {
                warn!("Failed to send EntityDiscovered message: {}", e);
            } else {
                info!("Registered light entity: {}", entity_id);
            }
        }
    }

    /// Report a state change to the engine
    fn report_state_change(&self, light_id: &str, state: &LightState) {
        if let Some(ref tx) = self.to_engine {
            let msg = crate::engine::Message::from_integration(
                crate::engine::MessagePayload::LightStateChanged {
                    entity_id: light_id.to_string(),
                    on: state.on,
                    brightness: state.brightness,
                },
            );
            if let Err(e) = tx.send(msg) {
                warn!("Failed to send LightStateChanged message: {}", e);
            }
        }
    }
}

#[async_trait]
impl<C: MqttClient> Integration for MqttIntegration<C> {
    async fn setup(
        &mut self,
        tx: crate::engine::MessageSender,
    ) -> Result<(), Box<dyn Error + Send>> {
        // Store MessageSender for sending messages to engine
        self.to_engine = Some(tx);

        // Connect to the MQTT broker
        info!(
            "Connecting to MQTT broker at {}:{}",
            self.config.broker, self.config.port
        );
        self.client.connect().await?;
        info!("Connected to MQTT broker");

        // Subscribe to discovery topics for lights
        let discovery_topic = format!("{}/light/+/+/config", self.config.discovery_prefix);
        info!("Subscribing to discovery topic: {}", discovery_topic);
        self.client.subscribe(&discovery_topic).await?;

        info!("MQTT integration setup complete, processing messages...");

        // Process messages (this will run until no more messages are available)
        self.process_messages().await
    }

    async fn handle_message(
        &mut self,
        msg: crate::engine::Message,
    ) -> Result<(), Box<dyn Error + Send>> {
        use crate::engine::MessagePayload;

        match msg.payload {
            MessagePayload::LightStateChanged {
                entity_id,
                on,
                brightness,
            } => {
                info!(
                    "Handling light command for {}: on={}, brightness={:?}",
                    entity_id, on, brightness
                );
                let state = LightState { on, brightness };
                self.send_light_command(&entity_id, state).await?;
            }
            _ => {
                // Ignore other message types (EntityDiscovered, EntityRemoved)
                debug!("Ignoring non-command message: {:?}", msg.payload);
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
impl TryFrom<&crate::config::MqttIntegrationConfig>
    for MqttIntegration<crate::integrations::mqtt::client::RumqttcClient>
{
    type Error = Box<dyn Error>;

    fn try_from(cfg: &crate::config::MqttIntegrationConfig) -> Result<Self, Self::Error> {
        let mqtt_config = MqttConfig {
            broker: cfg.broker.clone(),
            port: cfg.port,
            discovery_prefix: cfg.discovery_prefix.clone(),
            client_id: cfg.client_id.clone(),
            username: cfg.username.clone(),
            password: cfg.password.clone(),
        };

        let client = crate::integrations::mqtt::client::RumqttcClient::new(&mqtt_config)?;
        Ok(MqttIntegration::new(client, mqtt_config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::mqtt::client::MockMqttClient;

    #[tokio::test]
    async fn test_mqtt_integration_creation() {
        let client = MockMqttClient::new();
        let config = MqttConfig::default();
        let integration = MqttIntegration::new(client, config);

        assert_eq!(integration.lights.len(), 0);
    }
}
