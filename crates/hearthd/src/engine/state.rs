use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

/// State of a light entity.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct LightState {
    /// Whether the light is on or off.
    pub on: bool,

    /// Brightness level (0-255), if supported.
    pub brightness: Option<u8>,
}

/// State of a binary sensor entity.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct BinarySensorState {
    /// Whether the sensor is active (meaning depends on device class:
    /// motion detected, door open, tamper triggered, etc.)
    pub on: bool,
}

/// Centralized snapshot of the entire engine state.
///
/// This is the `State` that automations receive as their second argument.
#[derive(Debug, Clone, Default, Serialize, facet::Facet)]
pub struct State {
    pub lights: HashMap<String, LightState>,
    pub binary_sensors: HashMap<String, BinarySensorState>,
}
