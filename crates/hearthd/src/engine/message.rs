//! Type-safe message system for hearthd
//!
//! Messages are split by direction to enforce correct usage at compile time:
//! - `FromIntegrationMessage`: Events from integrations to the engine
//! - `ToIntegrationMessage`: Commands from the engine to integrations

/// Device info forwarded from HA integrations.
#[derive(Debug, Clone)]
pub struct HaDeviceInfo {
    pub identifiers: Vec<Vec<String>>,
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub sw_version: Option<String>,
}

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

    /// HA entity registered with metadata
    HaEntityRegistered {
        entity_id: String,
        name: String,
        platform: String,
        device_class: Option<String>,
        capabilities: Option<serde_json::Value>,
        device_info: Option<HaDeviceInfo>,
        integration_name: String,
    },

    /// HA state update with generic JSON attributes
    HaStateUpdated {
        entity_id: String,
        state: String,
        attributes: serde_json::Value,
        last_updated: String,
    },
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
            FromIntegrationMessage::HaEntityRegistered {
                entity_id,
                name,
                platform,
                integration_name,
                ..
            } => f
                .debug_struct("HaEntityRegistered")
                .field("entity_id", entity_id)
                .field("name", name)
                .field("platform", platform)
                .field("integration_name", integration_name)
                .finish(),
            FromIntegrationMessage::HaStateUpdated {
                entity_id,
                state,
                last_updated,
                ..
            } => f
                .debug_struct("HaStateUpdated")
                .field("entity_id", entity_id)
                .field("state", state)
                .field("last_updated", last_updated)
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
