//! Runtime coordination for Home Assistant integrations.

use super::protocol::Message;
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
}

#[derive(Debug, Clone)]
struct IntegrationState {
    domain: String,
    loaded: bool,
}

impl Runtime {
    /// Create a new runtime instance
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            integrations: HashMap::new(),
        }
    }

    /// Handle a message from the Python sandbox
    pub fn handle_message(&mut self, message: Message) {
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
            }
            Message::SetupComplete { entry_id, platforms: _ } => {
                // TODO: Track entry_id instead of domain
                tracing::info!("Integration {} setup complete", entry_id);
            }
            Message::SetupFailed { entry_id, error } => {
                tracing::error!("Integration {} failed to load: {}", entry_id, error);
            }
            Message::Log { level, logger, message } => {
                use super::protocol::LogLevel;
                match level {
                    LogLevel::Debug => tracing::debug!("[{}] {}", logger, message),
                    LogLevel::Info => tracing::info!("[{}] {}", logger, message),
                    LogLevel::Warning => tracing::warn!("[{}] {}", logger, message),
                    LogLevel::Error => tracing::error!("[{}] {}", logger, message),
                }
            }
            Message::Ready => {
                tracing::info!("Python sandbox is ready");
            }
            // TODO: Handle remaining message types
            Message::HttpRequest { .. }
            | Message::ScheduleUpdate { .. }
            | Message::CancelTimer { .. }
            | Message::GetConfig { .. }
            | Message::UnloadComplete { .. }
            | Message::UpdateComplete { .. } => {
                tracing::warn!("Unhandled message type: {:?}", message);
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

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}
