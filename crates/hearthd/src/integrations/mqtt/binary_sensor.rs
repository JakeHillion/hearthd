use std::error::Error;
use std::fmt;

use serde::Deserialize;
use serde::Serialize;

use crate::engine::Entity;
use crate::integrations::mqtt::discovery::DeviceInfo;
use crate::integrations::mqtt::discovery::DiscoveryMessage;

/// Device class for binary sensors, matching Home Assistant's binary_sensor device classes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinarySensorDeviceClass {
    Battery,
    BatteryCharging,
    CarbonMonoxide,
    Cold,
    Connectivity,
    Door,
    GarageDoor,
    Gas,
    Heat,
    Light,
    Lock,
    Moisture,
    Motion,
    Moving,
    Occupancy,
    Opening,
    Plug,
    Power,
    Presence,
    Problem,
    Running,
    Safety,
    Smoke,
    Sound,
    Tamper,
    Update,
    Vibration,
    Window,
    /// A device class not yet known to hearthd
    Unknown(String),
}

impl fmt::Display for BinarySensorDeviceClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(s) => write!(f, "{}", s),
            other => {
                // Use the Debug repr lowercased with underscores via serde
                let json = serde_json::to_value(other).unwrap();
                write!(f, "{}", json.as_str().unwrap())
            }
        }
    }
}

impl From<String> for BinarySensorDeviceClass {
    fn from(s: String) -> Self {
        match s.as_str() {
            "battery" => Self::Battery,
            "battery_charging" => Self::BatteryCharging,
            "carbon_monoxide" => Self::CarbonMonoxide,
            "cold" => Self::Cold,
            "connectivity" => Self::Connectivity,
            "door" => Self::Door,
            "garage_door" => Self::GarageDoor,
            "gas" => Self::Gas,
            "heat" => Self::Heat,
            "light" => Self::Light,
            "lock" => Self::Lock,
            "moisture" => Self::Moisture,
            "motion" => Self::Motion,
            "moving" => Self::Moving,
            "occupancy" => Self::Occupancy,
            "opening" => Self::Opening,
            "plug" => Self::Plug,
            "power" => Self::Power,
            "presence" => Self::Presence,
            "problem" => Self::Problem,
            "running" => Self::Running,
            "safety" => Self::Safety,
            "smoke" => Self::Smoke,
            "sound" => Self::Sound,
            "tamper" => Self::Tamper,
            "update" => Self::Update,
            "vibration" => Self::Vibration,
            "window" => Self::Window,
            _ => Self::Unknown(s),
        }
    }
}

/// State of a binary sensor entity
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BinarySensorState {
    /// Whether the sensor is active (meaning depends on device class:
    /// motion detected, door open, tamper triggered, etc.)
    pub on: bool,
}

/// Binary sensor entity (e.g., motion/occupancy sensor)
#[derive(Debug, Clone)]
pub struct BinarySensor {
    /// Entity ID (e.g., "binary_sensor.living_room")
    #[allow(dead_code)]
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Unique identifier from Zigbee2MQTT
    #[allow(dead_code)]
    pub unique_id: String,

    /// Device class (e.g., Motion, Occupancy)
    #[allow(dead_code)]
    pub device_class: Option<BinarySensorDeviceClass>,

    /// Current state
    pub state: BinarySensorState,

    /// Device information
    #[allow(dead_code)]
    pub device_info: Option<DeviceInfo>,

    /// Topic to receive state updates
    pub state_topic: String,

    /// Value template for extracting state from JSON payload
    /// e.g., "{{ value_json.occupancy }}" -> key is "occupancy"
    value_template: Option<String>,
}

/// Extract the JSON key name from a Zigbee2MQTT value template.
///
/// Parses templates like `{{ value_json.occupancy }}` and returns `"occupancy"`.
/// Returns `None` if the template doesn't match the expected format.
fn parse_value_template_key(template: &str) -> Option<&str> {
    let inner = template
        .trim()
        .strip_prefix("{{")?
        .strip_suffix("}}")?
        .trim();
    inner.strip_prefix("value_json.")
}

impl BinarySensor {
    /// Create a BinarySensor entity from a Zigbee2MQTT discovery message
    pub fn from_discovery(
        discovery: DiscoveryMessage,
        id: String,
        node_id: String,
    ) -> Result<Self, Box<dyn Error>> {
        let unique_id = discovery
            .unique_id
            .unwrap_or_else(|| format!("{}_binary_sensor", node_id));

        let name = discovery
            .name
            .unwrap_or_else(|| format!("Binary Sensor {}", node_id));

        let state_topic = discovery
            .state_topic
            .ok_or("Missing state_topic in discovery message")?;

        let device_class = discovery.device_class.map(BinarySensorDeviceClass::from);

        Ok(Self {
            id,
            name,
            unique_id,
            device_class,
            state: BinarySensorState::default(),
            device_info: discovery.device,
            state_topic,
            value_template: discovery.value_template,
        })
    }

    /// Update the binary sensor state from an MQTT payload
    ///
    /// Zigbee2MQTT sends state updates as JSON, e.g.:
    /// {"occupancy": true, "battery": 100, "illuminance": 42, "linkquality": 120}
    pub fn update_state(&mut self, payload: &[u8]) -> Result<(), Box<dyn Error>> {
        let json_str = std::str::from_utf8(payload)?;
        let state_update: serde_json::Value = serde_json::from_str(json_str)?;

        // Determine which JSON key holds the occupancy state
        let key = self
            .value_template
            .as_deref()
            .and_then(parse_value_template_key)
            .unwrap_or("state");

        // Extract occupancy from the determined key
        if let Some(value) = state_update.get(key) {
            self.state.on = match value {
                serde_json::Value::Bool(b) => *b,
                serde_json::Value::String(s) => s == "ON" || s == "true",
                _ => false,
            };
        }

        Ok(())
    }
}

impl Entity for BinarySensor {
    fn state_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_sensor_state_default() {
        let state = BinarySensorState::default();
        assert!(!state.on);
    }

    #[test]
    fn test_from_discovery() {
        let discovery = DiscoveryMessage {
            name: Some("Living Room Motion".to_string()),
            unique_id: Some("0x00124b001234abcd_occupancy".to_string()),
            state_topic: Some("zigbee2mqtt/motion_sensor".to_string()),
            command_topic: None,
            brightness_state_topic: None,
            brightness_command_topic: None,
            device: None,
            payload_on: None,
            payload_off: None,
            brightness: None,
            schema: None,
            device_class: Some("motion".to_string()),
            value_template: Some("{{ value_json.occupancy }}".to_string()),
        };

        let sensor = BinarySensor::from_discovery(
            discovery,
            "binary_sensor.living_room".to_string(),
            "living_room".to_string(),
        )
        .unwrap();

        assert_eq!(sensor.name, "Living Room Motion");
        assert_eq!(sensor.unique_id, "0x00124b001234abcd_occupancy");
        assert_eq!(sensor.device_class, Some(BinarySensorDeviceClass::Motion));
        assert_eq!(sensor.state_topic, "zigbee2mqtt/motion_sensor");
        assert!(!sensor.state.on);
    }

    #[test]
    fn test_from_discovery_missing_state_topic() {
        let discovery = DiscoveryMessage {
            name: Some("Test".to_string()),
            unique_id: None,
            state_topic: None,
            command_topic: None,
            brightness_state_topic: None,
            brightness_command_topic: None,
            device: None,
            payload_on: None,
            payload_off: None,
            brightness: None,
            schema: None,
            device_class: None,
            value_template: None,
        };

        let result = BinarySensor::from_discovery(
            discovery,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_update_state_occupancy_true() {
        let discovery = DiscoveryMessage {
            name: Some("Motion".to_string()),
            unique_id: Some("test_sensor".to_string()),
            state_topic: Some("zigbee2mqtt/sensor".to_string()),
            command_topic: None,
            brightness_state_topic: None,
            brightness_command_topic: None,
            device: None,
            payload_on: None,
            payload_off: None,
            brightness: None,
            schema: None,
            device_class: Some("motion".to_string()),
            value_template: Some("{{ value_json.occupancy }}".to_string()),
        };

        let mut sensor = BinarySensor::from_discovery(
            discovery,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        let payload =
            br#"{"occupancy": true, "battery": 95, "illuminance": 42, "linkquality": 120}"#;
        sensor.update_state(payload).unwrap();

        assert!(sensor.state.on);
    }

    #[test]
    fn test_update_state_occupancy_false() {
        let discovery = DiscoveryMessage {
            name: Some("Motion".to_string()),
            unique_id: Some("test_sensor".to_string()),
            state_topic: Some("zigbee2mqtt/sensor".to_string()),
            command_topic: None,
            brightness_state_topic: None,
            brightness_command_topic: None,
            device: None,
            payload_on: None,
            payload_off: None,
            brightness: None,
            schema: None,
            device_class: Some("motion".to_string()),
            value_template: Some("{{ value_json.occupancy }}".to_string()),
        };

        let mut sensor = BinarySensor::from_discovery(
            discovery,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        let payload = br#"{"occupancy": false, "battery": 100}"#;
        sensor.update_state(payload).unwrap();

        assert!(!sensor.state.on);
    }

    #[test]
    fn test_update_state_fallback_to_state_key() {
        let discovery = DiscoveryMessage {
            name: Some("Sensor".to_string()),
            unique_id: Some("test".to_string()),
            state_topic: Some("zigbee2mqtt/sensor".to_string()),
            command_topic: None,
            brightness_state_topic: None,
            brightness_command_topic: None,
            device: None,
            payload_on: None,
            payload_off: None,
            brightness: None,
            schema: None,
            device_class: None,
            value_template: None,
        };

        let mut sensor = BinarySensor::from_discovery(
            discovery,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        let payload = br#"{"state": "ON"}"#;
        sensor.update_state(payload).unwrap();

        assert!(sensor.state.on);
    }

    #[test]
    fn test_parse_value_template_key() {
        assert_eq!(
            parse_value_template_key("{{ value_json.occupancy }}"),
            Some("occupancy")
        );
        assert_eq!(
            parse_value_template_key("{{value_json.contact}}"),
            Some("contact")
        );
        assert_eq!(parse_value_template_key("invalid"), None);
        assert_eq!(parse_value_template_key("{{ something_else }}"), None);
    }

    #[test]
    fn test_state_json() {
        let state = BinarySensorState { on: true };
        let sensor = BinarySensor {
            id: "binary_sensor.test".to_string(),
            name: "Test".to_string(),
            unique_id: "test".to_string(),
            device_class: Some(BinarySensorDeviceClass::Motion),
            state,
            device_info: None,
            state_topic: "test".to_string(),
            value_template: None,
        };

        let json = sensor.state_json();
        assert_eq!(json["on"], true);
    }
}
