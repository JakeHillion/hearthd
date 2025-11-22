/// Unified message system for hearthd
///
/// Instead of separate Event and Command types, we use a single Message type
/// with a Direction field. This makes it easy for automations to transform
/// incoming messages into outgoing commands without explicit type conversions.
///
/// Direction indicates whether this message is informational or actionable
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Read from integration - informing engine of real-world state
    FromIntegration,
    /// Write to integration - requesting integration to change real-world state
    ToIntegration,
}

/// Unified message type for all entity state changes and commands
#[derive(Debug)]
pub struct Message {
    pub direction: Direction,
    pub payload: MessagePayload,
}

/// Payload types for different kinds of messages
pub enum MessagePayload {
    /// An entity was discovered and registered
    EntityDiscovered {
        entity_id: String,
        entity: std::sync::Arc<tokio::sync::Mutex<dyn super::Entity>>,
    },

    /// An entity was removed (device unplugged, etc.)
    EntityRemoved { entity_id: String },

    /// A light's state changed (or command to change it)
    LightStateChanged {
        entity_id: String,
        on: bool,
        brightness: Option<u8>,
    },
    // Future additions:
    // SwitchStateChanged { entity_id: String, on: bool },
    // SensorReading { entity_id: String, value: f64, unit: String },
    // ClimateStateChanged { entity_id: String, temperature: f64, mode: ClimateMode },
}

impl std::fmt::Debug for MessagePayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessagePayload::EntityDiscovered { entity_id, .. } => {
                f.debug_struct("EntityDiscovered")
                    .field("entity_id", entity_id)
                    .field("entity", &"<entity>")
                    .finish()
            }
            MessagePayload::EntityRemoved { entity_id } => f
                .debug_struct("EntityRemoved")
                .field("entity_id", entity_id)
                .finish(),
            MessagePayload::LightStateChanged {
                entity_id,
                on,
                brightness,
            } => f
                .debug_struct("LightStateChanged")
                .field("entity_id", entity_id)
                .field("on", on)
                .field("brightness", brightness)
                .finish(),
        }
    }
}

impl Message {
    /// Create a message from an integration (informational)
    pub fn from_integration(payload: MessagePayload) -> Self {
        Self {
            direction: Direction::FromIntegration,
            payload,
        }
    }

    /// Create a message to an integration (command)
    pub fn to_integration(payload: MessagePayload) -> Self {
        Self {
            direction: Direction::ToIntegration,
            payload,
        }
    }

    /// Check if this message is actionable (should be sent to integration)
    pub fn is_actionable(&self) -> bool {
        self.direction == Direction::ToIntegration
    }

    /// Check if this message is informational (came from integration)
    pub fn is_informational(&self) -> bool {
        self.direction == Direction::FromIntegration
    }
}
