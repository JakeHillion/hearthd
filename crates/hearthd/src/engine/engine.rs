use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::error;
use tracing::info;
use tracing::warn;

use super::event::Event;
use super::integration::FromIntegrationReceiver;
use super::integration::FromIntegrationSender;
use super::integration::Integration;
use super::integration::ToIntegrationSender;
use super::message::FromIntegrationMessage;
use super::message::ToIntegrationMessage;
use super::state::BinarySensorState;
use super::state::LightState;
use super::state::State;
use crate::engine::IntegrationContext;

/// hearthd engine
///
/// This structure handles the flow of events, applying automations to them, sending them to the
/// correct integration, and maintaining a view of the world with State.
pub struct Engine {
    /// Centralized state snapshot (readers load the Arc, writer stores a new one)
    state: ArcSwap<State>,

    /// Map of entity_id -> integration name for routing messages
    entity_integration_map: std::sync::Mutex<HashMap<String, String>>,

    /// Communication channels to integrations (for commands)
    integration_channels: HashMap<String, ToIntegrationSender>,

    /// Receive messages from integrations (events)
    message_rx: Mutex<FromIntegrationReceiver>,

    /// Sender for integrations to report events back to the engine
    message_tx: FromIntegrationSender,

    /// Handles for integration tasks
    integration_handles: Vec<JoinHandle<()>>,
}

/// Capacity for the integration→engine message channel
/// Provides backpressure when integrations send faster than the engine can process
const FROM_INTEGRATION_CHANNEL_SIZE: usize = 1024;

impl Engine {
    /// Create a new Engine instance
    pub fn new() -> Self {
        let (message_tx, message_rx) = mpsc::channel(FROM_INTEGRATION_CHANNEL_SIZE);
        Self {
            state: ArcSwap::new(Arc::default()),
            entity_integration_map: std::sync::Mutex::new(HashMap::new()),
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
        let ctx = IntegrationContext { config: cfg };
        for constr in super::integration::REGISTRY {
            let integration = match constr(&ctx) {
                Ok(Some(i)) => i,
                Err(e) => {
                    error!("failed to setup integration: {}", e);
                    continue;
                }
                Ok(None) => continue,
            };
            let name = integration.name().to_string();
            self.register_integration(name, integration);
        }

        Ok(())
    }

    /// Register an integration with the engine
    ///
    /// This spawns the integration in a background task, wires up channels,
    /// and starts its setup process.
    pub fn register_integration(&mut self, name: String, mut integration: Box<dyn Integration>) {
        let (to_integration_tx, mut to_integration_rx) = mpsc::unbounded_channel();
        let from_integration_tx = self.message_tx.clone();

        self.integration_channels
            .insert(name.clone(), to_integration_tx);

        // Spawn integration task
        let handle = tokio::spawn(async move {
            // Setup integration (gives it the sender for events)
            if let Err(e) = integration.setup(from_integration_tx).await {
                warn!("Integration '{}' setup failed: {}", name, e);
                return;
            }

            // Process commands from engine
            while let Some(msg) = to_integration_rx.recv().await {
                if let Err(e) = integration.handle_message(msg).await {
                    warn!("Integration '{}' failed to handle message: {}", name, e);
                }
            }

            if let Err(e) = integration.shutdown().await {
                warn!("Integration '{}' shutdown failed: {}", name, e);
            }
        });

        self.integration_handles.push(handle);
    }

    /// Send a command to an integration
    ///
    /// Routes the command to the appropriate integration based on entity_id.
    pub fn send_command(&self, msg: ToIntegrationMessage) -> Result<(), Box<dyn Error + Send>> {
        // Extract entity_id from command for routing
        let entity_id = match &msg {
            ToIntegrationMessage::LightCommand { entity_id, .. } => entity_id.clone(),
        };

        // Route to the integration that owns this entity
        let map = self
            .entity_integration_map
            .lock()
            .map_err(|e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::other(e.to_string()))
            })?;

        let integration_name = map
            .get(&entity_id)
            .ok_or_else(|| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("No integration found for entity: {}", entity_id),
                ))
            })?;

        let tx = self.integration_channels.get(integration_name).ok_or_else(
            || -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Integration channel not found: {}", integration_name),
                ))
            },
        )?;

        tx.send(msg)
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })
    }

    /// Run the engine's main event loop
    ///
    /// Processes incoming events from integrations and updates state.
    pub async fn run(&self) -> Result<(), Box<dyn Error + Send>> {
        info!("Engine starting");

        // Main event loop - only receives FromIntegration messages
        let mut rx = self.message_rx.lock().await;
        while let Some(msg) = rx.recv().await {
            if let Err(e) = self.handle_event(msg).await {
                warn!("Error handling event: {}", e);
            }
        }

        info!("Engine shutting down");
        Ok(())
    }

    /// Get a snapshot of the current engine state.
    ///
    /// Clones the `Arc` (atomic refcount bump), essentially free.
    pub fn state_snapshot(&self) -> Arc<State> {
        self.state.load_full()
    }

    /// Send a light command to control a light entity
    pub fn send_light_command(
        &self,
        entity_id: String,
        on: bool,
        brightness: Option<u8>,
    ) -> Result<(), Box<dyn Error + Send>> {
        let cmd = ToIntegrationMessage::LightCommand {
            entity_id,
            on,
            brightness,
        };
        self.send_command(cmd)
    }

    /// Handle an event from an integration
    async fn handle_event(&self, msg: FromIntegrationMessage) -> Result<(), Box<dyn Error + Send>> {
        match msg {
            FromIntegrationMessage::EntityDiscovered {
                entity_id,
                integration_name,
            } => {
                info!(
                    "Entity discovered: {} (from {})",
                    entity_id, integration_name
                );

                // Record which integration owns this entity for command routing.
                // State is not populated until the first state-change message arrives.
                if let Ok(mut map) = self.entity_integration_map.lock() {
                    map.insert(entity_id, integration_name);
                }
            }
            FromIntegrationMessage::EntityRemoved { entity_id } => {
                info!("Entity removed: {}", entity_id);

                {
                    let mut state = State::clone(&self.state.load());
                    state.lights.remove(&entity_id);
                    state.binary_sensors.remove(&entity_id);
                    self.state.store(Arc::new(state));
                }

                // Remove from routing map
                if let Ok(mut map) = self.entity_integration_map.lock() {
                    map.remove(&entity_id);
                }
            }
            FromIntegrationMessage::LightStateChanged {
                entity_id,
                on,
                brightness,
            } => {
                let light_state = LightState { on, brightness };
                info!(
                    "Light state changed: {} -> on={}, brightness={:?}",
                    entity_id, on, brightness
                );

                {
                    let mut state = State::clone(&self.state.load());
                    state.lights.insert(entity_id.clone(), light_state.clone());
                    self.state.store(Arc::new(state));
                }

                let _event = Event::LightStateChanged {
                    entity_id,
                    state: light_state,
                };
                // TODO: Trigger automations based on state change
            }
            FromIntegrationMessage::BinarySensorStateChanged { entity_id, on } => {
                let sensor_state = BinarySensorState { on };
                info!("Binary sensor state changed: {} -> on={}", entity_id, on);

                {
                    let mut state = State::clone(&self.state.load());
                    state
                        .binary_sensors
                        .insert(entity_id.clone(), sensor_state.clone());
                    self.state.store(Arc::new(state));
                }

                let _event = Event::BinarySensorStateChanged {
                    entity_id,
                    state: sensor_state,
                };
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
