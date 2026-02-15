use super::state::BinarySensorState;
use super::state::LightState;

/// Automation-level events.
///
/// Distinct from `FromIntegrationMessage` (transport-level). The engine converts
/// `FromIntegrationMessage` into `Event` at the boundary.
#[derive(Debug, Clone)]
pub enum Event {
    LightStateChanged {
        entity_id: String,
        state: LightState,
    },
    BinarySensorStateChanged {
        entity_id: String,
        state: BinarySensorState,
    },
}
