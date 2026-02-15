use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

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
use crate::engine::FromIntegrationMessage;
use crate::engine::FromIntegrationSender;
use crate::engine::Integration;
use crate::engine::ToIntegrationMessage;
use crate::engine::state::BinarySensorState;
use crate::engine::state::LightState;

/// Type alias for the shared lights map
type LightsMap = Arc<Mutex<HashMap<String, Arc<Mutex<Light>>>>>;

/// Type alias for the shared binary sensors map
type BinarySensorsMap = Arc<Mutex<HashMap<String, Arc<Mutex<BinarySensor>>>>>;

/// MQTT Integration for hearthd
///
/// Handles MQTT communication with Zigbee2MQTT and other MQTT-based devices.
/// Currently supports Light entities as MVP.
pub struct MqttIntegration<C: MqttClient> {
    client: Arc<Mutex<C>>,
    config: MqttConfig,
    lights: LightsMap,
    binary_sensors: BinarySensorsMap,
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
            lights: Arc::new(Mutex::new(HashMap::new())) as LightsMap,
            binary_sensors: Arc::new(Mutex::new(HashMap::new())) as BinarySensorsMap,
            to_engine: None,
            _message_task: None,
        }
    }

    /// Process incoming MQTT messages in a background task
    ///
    /// This is spawned as a separate tokio task in setup() so that
    /// handle_message() can process commands concurrently.
    async fn process_messages_task(
        client: Arc<Mutex<C>>,
        config: MqttConfig,
        lights: LightsMap,
        binary_sensors: BinarySensorsMap,
        to_engine: FromIntegrationSender,
    ) {
        loop {
            // Poll for message with a short lock hold time
            // Use tokio::select with a timeout to avoid holding the lock indefinitely
            let msg = {
                let mut client_guard = client.lock().await;
                // Use tokio timeout to avoid blocking forever while holding the lock
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
                        if let Err(e) = Self::handle_discovery_static(
                            &msg,
                            &config,
                            &client,
                            &lights,
                            &binary_sensors,
                            &to_engine,
                        )
                        .await
                        {
                            warn!("Error handling discovery message: {}", e);
                        }
                    } else if let Err(e) =
                        Self::handle_state_update_static(&msg, &lights, &binary_sensors, &to_engine)
                            .await
                    {
                        warn!("Error handling state update: {}", e);
                    }
                }
                None => {
                    // No message available, yield to allow other tasks (like command handling)
                    tokio::task::yield_now().await;
                }
            }
        }
    }

    /// Handle a discovery message (static version for background task)
    async fn handle_discovery_static(
        msg: &MqttMessage,
        config: &MqttConfig,
        client: &Arc<Mutex<C>>,
        lights: &LightsMap,
        binary_sensors: &BinarySensorsMap,
        to_engine: &FromIntegrationSender,
    ) -> Result<(), Box<dyn Error + Send>> {
        // Parse the discovery topic
        let (component, node_id, object_id) =
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
            component, node_id, object_id
        );

        match component.as_str() {
            "light" => Self::handle_light_discovery(msg, client, lights, to_engine, &node_id).await,
            "binary_sensor" => {
                // TODO: Zigbee2MQTT also publishes auxiliary data (battery,
                // illuminance, linkquality) as separate `sensor` components.
                // These should be discovered as numeric sensor entities.
                Self::handle_binary_sensor_discovery(
                    msg,
                    client,
                    binary_sensors,
                    to_engine,
                    &node_id,
                )
                .await
            }
            _ => {
                debug!("Ignoring unsupported component: {}", component);
                Ok(())
            }
        }
    }

    /// Handle discovery of a light entity
    async fn handle_light_discovery(
        msg: &MqttMessage,
        client: &Arc<Mutex<C>>,
        lights: &LightsMap,
        to_engine: &FromIntegrationSender,
        node_id: &str,
    ) -> Result<(), Box<dyn Error + Send>> {
        let entity_id = format!("light.{}", node_id);

        if msg.payload.is_empty() {
            let mut lights_guard = lights.lock().await;
            if lights_guard.remove(&entity_id).is_some() {
                info!("Removed light entity: {}", entity_id);
                Self::notify_entity_removed_static(&entity_id, to_engine).await;
            }
            return Ok(());
        }

        let discovery: DiscoveryMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })?;

        let light = Light::from_discovery(discovery, entity_id.clone(), node_id.to_string())
            .map_err(|e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ))
            })?;

        let state_topic = light.state_topic.clone();
        info!("Discovered light entity: {} ({})", light.name, entity_id);

        let light_arc = Arc::new(Mutex::new(light));

        {
            let mut lights_guard = lights.lock().await;
            lights_guard.insert(entity_id.clone(), light_arc.clone());
        }

        // Subscribe after map insert so the retained state message finds the
        // entity already in the map, regardless of concurrency model.
        {
            let mut client_guard = client.lock().await;
            client_guard.subscribe(&state_topic).await?;
        }

        Self::register_entity_static(&entity_id, to_engine).await;

        Ok(())
    }

    /// Handle discovery of a binary sensor entity (e.g., motion sensor)
    async fn handle_binary_sensor_discovery(
        msg: &MqttMessage,
        client: &Arc<Mutex<C>>,
        binary_sensors: &BinarySensorsMap,
        to_engine: &FromIntegrationSender,
        node_id: &str,
    ) -> Result<(), Box<dyn Error + Send>> {
        let entity_id = format!("binary_sensor.{}", node_id);

        if msg.payload.is_empty() {
            let mut sensors_guard = binary_sensors.lock().await;
            if sensors_guard.remove(&entity_id).is_some() {
                info!("Removed binary sensor entity: {}", entity_id);
                Self::notify_entity_removed_static(&entity_id, to_engine).await;
            }
            return Ok(());
        }

        let discovery: DiscoveryMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })?;

        let sensor =
            BinarySensor::from_discovery(discovery, entity_id.clone(), node_id.to_string())
                .map_err(|e| -> Box<dyn Error + Send> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    ))
                })?;

        let state_topic = sensor.state_topic.clone();
        info!(
            "Discovered binary sensor entity: {} ({})",
            sensor.name, entity_id
        );

        let sensor_arc = Arc::new(Mutex::new(sensor));

        {
            let mut sensors_guard = binary_sensors.lock().await;
            sensors_guard.insert(entity_id.clone(), sensor_arc.clone());
        }

        // Subscribe after map insert so the retained state message finds the
        // entity already in the map, regardless of concurrency model.
        {
            let mut client_guard = client.lock().await;
            client_guard.subscribe(&state_topic).await?;
        }

        Self::register_entity_static(&entity_id, to_engine).await;

        Ok(())
    }

    /// Handle a state update message (static version for background task)
    async fn handle_state_update_static(
        msg: &MqttMessage,
        lights: &LightsMap,
        binary_sensors: &BinarySensorsMap,
        to_engine: &FromIntegrationSender,
    ) -> Result<(), Box<dyn Error + Send>> {
        // Check lights first
        let mut light_to_update: Option<(String, LightState)> = None;

        {
            let lights_guard = lights.lock().await;
            for (entity_id, light_arc) in lights_guard.iter() {
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
                    light_to_update = Some((entity_id.clone(), light.state.clone()));
                    break;
                }
            }
        }

        if let Some((entity_id, state)) = light_to_update {
            Self::report_state_change_static(&entity_id, &state, to_engine).await;
            return Ok(());
        }

        // Check binary sensors
        let mut sensor_to_update: Option<(String, BinarySensorState)> = None;

        {
            let sensors_guard = binary_sensors.lock().await;
            for (entity_id, sensor_arc) in sensors_guard.iter() {
                let mut sensor = sensor_arc.lock().await;
                if msg.topic == sensor.state_topic {
                    debug!("State update for binary sensor: {}", entity_id);
                    sensor
                        .update_state(&msg.payload)
                        .map_err(|e| -> Box<dyn Error + Send> {
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                e.to_string(),
                            ))
                        })?;
                    sensor_to_update = Some((entity_id.clone(), sensor.state.clone()));
                    break;
                }
            }
        }

        if let Some((entity_id, state)) = sensor_to_update {
            Self::report_binary_sensor_state_change_static(&entity_id, &state, to_engine).await;
        }

        Ok(())
    }

    /// Register an entity with the engine (static version)
    async fn register_entity_static(entity_id: &str, to_engine: &FromIntegrationSender) {
        let msg = FromIntegrationMessage::EntityDiscovered {
            entity_id: entity_id.to_string(),
            integration_name: "mqtt".to_string(),
        };
        if let Err(e) = to_engine.send(msg).await {
            warn!("Failed to send EntityDiscovered message: {}", e);
        } else {
            info!("Registered entity: {}", entity_id);
        }
    }

    /// Notify the engine that an entity has been removed (static version)
    async fn notify_entity_removed_static(entity_id: &str, to_engine: &FromIntegrationSender) {
        let msg = FromIntegrationMessage::EntityRemoved {
            entity_id: entity_id.to_string(),
        };
        if let Err(e) = to_engine.send(msg).await {
            warn!("Failed to send EntityRemoved message: {}", e);
        } else {
            info!("Notified engine of entity removal: {}", entity_id);
        }
    }

    /// Report a state change to the engine (static version)
    async fn report_state_change_static(
        light_id: &str,
        state: &LightState,
        to_engine: &FromIntegrationSender,
    ) {
        let msg = FromIntegrationMessage::LightStateChanged {
            entity_id: light_id.to_string(),
            on: state.on,
            brightness: state.brightness,
        };
        if let Err(e) = to_engine.send(msg).await {
            warn!("Failed to send LightStateChanged message: {}", e);
        }
    }

    /// Report a binary sensor state change to the engine (static version)
    async fn report_binary_sensor_state_change_static(
        sensor_id: &str,
        state: &BinarySensorState,
        to_engine: &FromIntegrationSender,
    ) {
        let msg = FromIntegrationMessage::BinarySensorStateChanged {
            entity_id: sensor_id.to_string(),
            on: state.on,
        };
        if let Err(e) = to_engine.send(msg).await {
            warn!("Failed to send BinarySensorStateChanged message: {}", e);
        }
    }

    /// Send a command to a light
    pub async fn send_light_command(
        &self,
        light_id: &str,
        state: LightState,
    ) -> Result<(), Box<dyn Error + Send>> {
        let lights_guard = self.lights.lock().await;
        let light_arc = lights_guard
            .get(light_id)
            .ok_or_else(|| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Light not found: {}", light_id),
                ))
            })?
            .clone();
        drop(lights_guard);

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

        {
            let mut client = self.client.lock().await;
            client.publish(&command_topic, &payload, false).await?;
        }

        info!("Sent command to light {}: {:?}", light_id, state);

        Ok(())
    }
}

#[async_trait]
impl<C: MqttClient + 'static> Integration for MqttIntegration<C> {
    fn name(&self) -> &str {
        "mqtt"
    }

    async fn setup(&mut self, tx: FromIntegrationSender) -> Result<(), Box<dyn Error + Send>> {
        // Store sender for sending events to engine
        self.to_engine = Some(tx.clone());

        // Connect to the MQTT broker
        info!(
            "Connecting to MQTT broker at {}:{}",
            self.config.broker, self.config.port
        );
        {
            let mut client = self.client.lock().await;
            client.connect().await?;
        }
        info!("Connected to MQTT broker");

        // Subscribe to discovery topics for lights and binary sensors
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

        // Clone shared state for the background task
        let client = self.client.clone();
        let config = self.config.clone();
        let lights = self.lights.clone();
        let binary_sensors = self.binary_sensors.clone();

        // Spawn background task to process incoming MQTT messages
        let task = tokio::spawn(async move {
            Self::process_messages_task(client, config, lights, binary_sensors, tx).await;
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
            ToIntegrationMessage::LightCommand {
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
    async fn test_mqtt_integration_creation() {
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

        let lights = integration.lights.lock().await;
        assert_eq!(lights.len(), 0);

        let binary_sensors = integration.binary_sensors.lock().await;
        assert_eq!(binary_sensors.len(), 0);
    }
}
