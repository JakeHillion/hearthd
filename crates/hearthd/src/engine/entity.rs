/// Entity abstraction for hearthd
///
/// All entities (lights, switches, sensors, etc.) implement the Entity trait.
///
/// Base trait that all entities must implement
pub trait Entity: Send + Sync {
    /// Serialize current state to JSON for Engine storage
    fn state_json(&self) -> serde_json::Value;
}
