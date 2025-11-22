use super::entity::Entity;
use super::integration::{Integration, MessageReceiver, MessageSender};
use super::message::{Direction, Message, MessagePayload};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::error::Error;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// hearthd engine
///
/// This structure handles the flow of events, applying automations to them, sending them to the
/// correct integration, and maintaining a view of the world with State.
pub struct Engine {
    /// Registry of all known entities (shared with integrations via Arc<Mutex>)
    entities: Mutex<HashMap<String, std::sync::Arc<Mutex<dyn Entity>>>>,

    /// Communication channels to integrations (for ToIntegration messages)
    integration_channels: HashMap<String, MessageSender>,

    /// Receive messages from integrations (FromIntegration messages)
    message_rx: Mutex<MessageReceiver>,

    /// Send messages to self (for routing)
    message_tx: MessageSender,

    /// Handles for integration tasks
    integration_handles: Vec<JoinHandle<()>>,
}

impl Engine {
    /// Create a new Engine instance
    pub fn new() -> Self {
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        Self {
            entities: Mutex::new(HashMap::new()),
            integration_channels: HashMap::new(),
            message_rx: Mutex::new(message_rx),
            message_tx,
            integration_handles: Vec::new(),
        }
    }

    /// Register integrations from configuration
    ///
    /// This is a convenience method that checks the config and registers
    /// any enabled integrations.
    pub fn register_integrations_from_config(
        &mut self,
        cfg: &crate::config::Config,
    ) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(feature = "integration_mqtt")]
        {
            if let Some(mqtt_cfg) = &cfg.integrations.mqtt {
                info!("MQTT integration is configured and enabled");
                use crate::integrations::mqtt::MqttIntegration;

                let mqtt_integration = MqttIntegration::try_from(mqtt_cfg)
                    .map_err(|e| format!("Failed to create MQTT integration: {}", e))?;

                self.register_integration("mqtt".to_string(), mqtt_integration);
            }
        }

        Ok(())
    }

    /// Register an integration with the engine
    ///
    /// This spawns the integration in a background task, wires up channels,
    /// and starts its setup process.
    pub fn register_integration<I: Integration + 'static>(
        &mut self,
        name: String,
        mut integration: I,
    ) {
        let (to_integration_tx, mut to_integration_rx) = mpsc::unbounded_channel();
        let from_integration_tx = self.message_tx.clone();

        self.integration_channels
            .insert(name.clone(), to_integration_tx);

        // Spawn integration task
        let handle = tokio::spawn(async move {
            // Setup integration (gives it the sender for FromIntegration messages)
            if let Err(e) = integration.setup(from_integration_tx).await {
                warn!("Integration '{}' setup failed: {}", name, e);
                return;
            }

            // Process ToIntegration messages
            while let Some(msg) = to_integration_rx.recv().await {
                if msg.direction == Direction::ToIntegration {
                    if let Err(e) = integration.handle_message(msg).await {
                        warn!("Integration '{}' failed to handle message: {}", name, e);
                    }
                }
            }

            if let Err(e) = integration.shutdown().await {
                warn!("Integration '{}' shutdown failed: {}", name, e);
            }
        });

        self.integration_handles.push(handle);
    }

    /// Send a message (typically ToIntegration direction)
    ///
    /// Routes the message to the appropriate integration based on entity_id.
    /// For now, sends to the first integration (assumes single integration).
    pub fn send_message(&self, msg: Message) -> Result<(), Box<dyn Error + Send>> {
        // TODO: More sophisticated routing based on which integration owns which entity
        // For now, just send to the first integration
        if let Some(tx) = self.integration_channels.values().next() {
            tx.send(msg)
                .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })?;
        }
        Ok(())
    }

    /// Run the engine's main event loop
    ///
    /// Processes incoming messages from integrations, updates state, and handles routing.
    pub async fn run(&self) -> Result<(), Box<dyn Error + Send>> {
        info!("Engine starting");

        // Main event loop
        let mut rx = self.message_rx.lock().await;
        while let Some(msg) = rx.recv().await {
            match msg.direction {
                Direction::FromIntegration => {
                    // Update internal state, run automations, etc.
                    if let Err(e) = self.handle_informational_message(msg).await {
                        warn!("Error handling informational message: {}", e);
                    }
                }
                Direction::ToIntegration => {
                    // Route to appropriate integration
                    if let Err(e) = self.send_message(msg) {
                        warn!("Error sending message to integration: {}", e);
                    }
                }
            }
        }

        info!("Engine shutting down");
        Ok(())
    }

    /// Get all entities as a JSON map
    pub async fn get_all_entities_json(&self) -> JsonValue {
        let entities = self.entities.lock().await;
        let mut entities_map = serde_json::Map::new();
        for (entity_id, entity_arc) in entities.iter() {
            let entity = entity_arc.lock().await;
            entities_map.insert(entity_id.clone(), entity.state_json());
        }
        JsonValue::Object(entities_map)
    }

    /// Get a specific entity's state as JSON
    pub async fn get_entity_json(&self, entity_id: &str) -> Option<JsonValue> {
        let entities = self.entities.lock().await;
        if let Some(entity_arc) = entities.get(entity_id) {
            let entity = entity_arc.lock().await;
            Some(entity.state_json())
        } else {
            None
        }
    }

    /// Get count of entities
    pub async fn entity_count(&self) -> usize {
        self.entities.lock().await.len()
    }

    /// Send a light command to control a light entity
    pub fn send_light_command(
        &self,
        entity_id: String,
        on: bool,
        brightness: Option<u8>,
    ) -> Result<(), Box<dyn Error + Send>> {
        let msg = Message::to_integration(MessagePayload::LightStateChanged {
            entity_id,
            on,
            brightness,
        });
        self.send_message(msg)
    }

    /// Handle a FromIntegration message (informational)
    async fn handle_informational_message(
        &self,
        msg: Message,
    ) -> Result<(), Box<dyn Error + Send>> {
        match msg.payload {
            MessagePayload::EntityDiscovered { entity_id, entity } => {
                info!("Entity discovered: {}", entity_id);
                let mut entities = self.entities.lock().await;
                entities.insert(entity_id, entity);
            }
            MessagePayload::EntityRemoved { entity_id } => {
                info!("Entity removed: {}", entity_id);
                let mut entities = self.entities.lock().await;
                entities.remove(&entity_id);
            }
            MessagePayload::LightStateChanged {
                entity_id,
                on,
                brightness,
            } => {
                info!(
                    "Light state changed: {} -> on={}, brightness={:?}",
                    entity_id, on, brightness
                );
                // Entity state is already updated by the integration
                // Engine just maintains the journal of state changes
                // TODO: Trigger automations based on state change
            }
        }
        Ok(())
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
