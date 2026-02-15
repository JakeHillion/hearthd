//! Type-safe message system for hearthd
//!
//! Messages are split by direction to enforce correct usage at compile time:
//! - `FromIntegrationMessage`: Events from integrations to the engine
//! - `ToIntegrationMessage`: Commands from the engine to integrations

/// Messages FROM integrations TO the engine (events/state updates)
pub enum FromIntegrationMessage {
    /// An entity was discovered and registered
    EntityDiscovered {
        entity_id: String,
        entity: std::sync::Arc<tokio::sync::Mutex<dyn super::Entity>>,
        integration_name: String,
    },

    /// An entity was removed (device unplugged, etc.)
    EntityRemoved { entity_id: String },

    /// A light's state changed
    LightStateChanged {
        entity_id: String,
        on: bool,
        brightness: Option<u8>,
    },

    /// A binary sensor's state changed (e.g., motion sensor)
    BinarySensorStateChanged { entity_id: String, on: bool },
}

impl std::fmt::Debug for FromIntegrationMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FromIntegrationMessage::EntityDiscovered {
                entity_id,
                integration_name,
                ..
            } => f
                .debug_struct("EntityDiscovered")
                .field("entity_id", entity_id)
                .field("integration_name", integration_name)
                .field("entity", &"<entity>")
                .finish(),
            FromIntegrationMessage::EntityRemoved { entity_id } => f
                .debug_struct("EntityRemoved")
                .field("entity_id", entity_id)
                .finish(),
            FromIntegrationMessage::LightStateChanged {
                entity_id,
                on,
                brightness,
            } => f
                .debug_struct("LightStateChanged")
                .field("entity_id", entity_id)
                .field("on", on)
                .field("brightness", brightness)
                .finish(),
            FromIntegrationMessage::BinarySensorStateChanged { entity_id, on } => f
                .debug_struct("BinarySensorStateChanged")
                .field("entity_id", entity_id)
                .field("on", on)
                .finish(),
        }
    }
}

/// Messages FROM the engine TO integrations (commands)
#[derive(Debug, Clone)]
pub enum ToIntegrationMessage {
    /// Command to change a light's state
    LightCommand {
        entity_id: String,
        on: bool,
        brightness: Option<u8>,
    },
}
