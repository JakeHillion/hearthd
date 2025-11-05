//! Runtime coordination for Home Assistant integrations.

use super::protocol::{Message, Response};
use crate::config::{LocationConfig, HaIntegrationConfig};
use std::collections::HashMap;

/// Entity state and metadata
#[derive(Debug, Clone)]
pub struct Entity {
    pub entity_id: String,
    pub platform: String,
    pub state: String,
    pub attributes: serde_json::Value,
}

/// Manages the runtime state of loaded integrations and entities.
pub struct Runtime {
    /// All registered entities, indexed by entity_id
    entities: HashMap<String, Entity>,

    /// Loaded integration domains
    integrations: HashMap<String, IntegrationState>,

    /// System location configuration
    location: LocationConfig,

    /// HA integration configurations, indexed by entry_id
    ha_configs: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
struct IntegrationState {
    domain: String,
    loaded: bool,
}

impl Runtime {
    /// Create a new runtime instance with location config
    pub fn new(location: LocationConfig) -> Self {
        Self {
            entities: HashMap::new(),
            integrations: HashMap::new(),
            location,
            ha_configs: HashMap::new(),
        }
    }

    /// Register an HA integration config
    pub fn register_ha_config(&mut self, entry_id: String, config: serde_json::Value) {
        self.ha_configs.insert(entry_id, config);
    }

    /// Handle a message from the Python sandbox
    /// Returns an optional response to send back
    pub fn handle_message(&mut self, message: Message) -> Option<Response> {
        match message {
            Message::EntityRegister {
                entry_id: _,
                entity_id,
                platform,
                device_class: _,
                capabilities: _,
                device_info: _,
            } => {
                self.entities.insert(
                    entity_id.clone(),
                    Entity {
                        entity_id,
                        platform,
                        state: "unknown".to_string(),
                        attributes: serde_json::Value::Object(Default::default()),
                    },
                );
                None
            }
            Message::StateUpdate {
                entity_id,
                state,
                attributes,
                last_updated: _,
            } => {
                if let Some(entity) = self.entities.get_mut(&entity_id) {
                    entity.state = state;
                    entity.attributes = attributes;
                }
                None
            }
            Message::SetupComplete { entry_id, platforms: _ } => {
                tracing::info!("Integration {} setup complete", entry_id);
                None
            }
            Message::SetupFailed { entry_id, error } => {
                tracing::error!("Integration {} failed to load: {}", entry_id, error);
                None
            }
            Message::Log { level, logger, message } => {
                use super::protocol::LogLevel;
                match level {
                    LogLevel::Debug => tracing::debug!("[{}] {}", logger, message),
                    LogLevel::Info => tracing::info!("[{}] {}", logger, message),
                    LogLevel::Warning => tracing::warn!("[{}] {}", logger, message),
                    LogLevel::Error => tracing::error!("[{}] {}", logger, message),
                }
                None
            }
            Message::Ready => {
                tracing::info!("Python sandbox is ready");
                None
            }
            Message::GetConfig { request_id, keys } => {
                let mut config = HashMap::new();

                for key in keys {
                    match key.as_str() {
                        // System location keys
                        "latitude" => {
                            config.insert(key, serde_json::json!(self.location.latitude));
                        }
                        "longitude" => {
                            config.insert(key, serde_json::json!(self.location.longitude));
                        }
                        "elevation" => {
                            config.insert(key, serde_json::json!(self.location.elevation));
                        }
                        "timezone" => {
                            config.insert(key, serde_json::json!(self.location.timezone));
                        }
                        // Integration-specific keys would be looked up from ha_configs
                        // For now, we'll handle this when we connect entry_id to requests
                        _ => {
                            tracing::warn!("Unknown config key requested: {}", key);
                        }
                    }
                }

                Some(Response::ConfigResponse { request_id, config })
            }
            // TODO: Handle remaining message types
            Message::HttpRequest { .. }
            | Message::ScheduleUpdate { .. }
            | Message::CancelTimer { .. }
            | Message::UnloadComplete { .. }
            | Message::UpdateComplete { .. } => {
                tracing::warn!("Unhandled message type: {:?}", message);
                None
            }
        }
    }

    /// Get an entity by ID
    pub fn get_entity(&self, entity_id: &str) -> Option<&Entity> {
        self.entities.get(entity_id)
    }

    /// Get all entities
    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }
}
