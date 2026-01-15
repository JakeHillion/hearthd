/// Entity abstraction for hearthd
///
/// All entities (lights, switches, sensors, etc.) implement the Entity trait.
///
/// Base trait that all entities must implement
pub trait Entity: Send + Sync {
    /// Serialize current state to JSON for Engine storage
    fn state_json(&self) -> serde_json::Value;

    /// Return the platform type of this entity (e.g. "weather", "light")
    fn platform(&self) -> &'static str;

    /// Update entity state from HA-style state/attributes.
    /// Default implementation does nothing; platform-specific entities override this.
    fn update_from_ha_state(&mut self, _state: &str, _attributes: &serde_json::Value) {}
}
