use std::error::Error;
use std::fmt;

use serde::Deserialize;
use serde::Serialize;

use crate::integrations::mqtt::discovery::DeviceInfo;
use crate::integrations::mqtt::discovery::DiscoveryMessage;
use crate::integrations::mqtt::light::Z2M_ENDPOINT;
use crate::matter::Cluster;
use crate::matter::Endpoint;
use crate::matter::Node;
use crate::matter::OccupancySensingCluster;

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

/// MQTT-side binary sensor (motion, occupancy, door, etc.).
///
/// Holds Z2M metadata plus the current OccupancySensing cluster state. The
/// device class is preserved for future use (e.g. choosing a different
/// Matter cluster for door sensors) but isn't yet surfaced through the
/// engine border.
#[derive(Debug, Clone)]
pub struct BinarySensor {
    pub entity_id: String,
    pub name: String,
    #[allow(dead_code)]
    pub unique_id: String,
    #[allow(dead_code)]
    pub device_class: Option<BinarySensorDeviceClass>,
    #[allow(dead_code)]
    pub device_info: Option<DeviceInfo>,

    pub state_topic: String,
    value_template: Option<String>,

    pub occupancy: OccupancySensingCluster,
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
    pub fn from_discovery(
        discovery: DiscoveryMessage,
        entity_id: String,
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
            entity_id,
            name,
            unique_id,
            device_class,
            device_info: discovery.device,
            state_topic,
            value_template: discovery.value_template,
            occupancy: OccupancySensingCluster::default(),
        })
    }

    /// Build the Matter `Node` snapshot for this sensor.
    pub fn to_node(&self, integration: &str) -> Node {
        let mut endpoint = Endpoint::default();
        endpoint.clusters.insert(
            crate::matter::CLUSTER_NAME_OCCUPANCY_SENSING.to_string(),
            Cluster::OccupancySensing(self.occupancy.clone()),
        );

        let mut endpoints = std::collections::HashMap::new();
        endpoints.insert(Z2M_ENDPOINT, endpoint);

        Node {
            entity_id: self.entity_id.clone(),
            integration: integration.to_string(),
            name: Some(self.name.clone()),
            endpoints,
        }
    }

    /// Apply an MQTT state-update payload and return the cluster snapshot
    /// if anything changed.
    pub fn apply_state_payload(
        &mut self,
        payload: &[u8],
    ) -> Result<Option<Cluster>, Box<dyn Error>> {
        let json_str = std::str::from_utf8(payload)?;
        let state_update: serde_json::Value = serde_json::from_str(json_str)?;

        let key = self
            .value_template
            .as_deref()
            .and_then(parse_value_template_key)
            .unwrap_or("state");

        if let Some(value) = state_update.get(key) {
            self.occupancy.occupancy = match value {
                serde_json::Value::Bool(b) => *b,
                serde_json::Value::String(s) => s == "ON" || s == "true",
                _ => false,
            };
            return Ok(Some(Cluster::OccupancySensing(self.occupancy.clone())));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motion_discovery() -> DiscoveryMessage {
        DiscoveryMessage {
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
        }
    }

    #[test]
    fn from_discovery_sets_defaults() {
        let sensor = BinarySensor::from_discovery(
            motion_discovery(),
            "binary_sensor.living_room".to_string(),
            "living_room".to_string(),
        )
        .unwrap();

        assert_eq!(sensor.name, "Living Room Motion");
        assert_eq!(sensor.device_class, Some(BinarySensorDeviceClass::Motion));
        assert_eq!(sensor.state_topic, "zigbee2mqtt/motion_sensor");
        assert!(!sensor.occupancy.occupancy);
    }

    #[test]
    fn from_discovery_rejects_missing_state_topic() {
        let mut discovery = motion_discovery();
        discovery.state_topic = None;
        let result = BinarySensor::from_discovery(
            discovery,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn apply_state_payload_updates_occupancy() {
        let mut sensor = BinarySensor::from_discovery(
            motion_discovery(),
            "binary_sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        let changed = sensor
            .apply_state_payload(br#"{"occupancy": true, "battery": 95}"#)
            .unwrap();
        assert!(matches!(changed, Some(Cluster::OccupancySensing(_))));
        assert!(sensor.occupancy.occupancy);

        let changed = sensor
            .apply_state_payload(br#"{"occupancy": false}"#)
            .unwrap();
        assert!(matches!(changed, Some(Cluster::OccupancySensing(_))));
        assert!(!sensor.occupancy.occupancy);
    }

    #[test]
    fn apply_state_payload_falls_back_to_state_key() {
        let mut discovery = motion_discovery();
        discovery.value_template = None;
        let mut sensor = BinarySensor::from_discovery(
            discovery,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        sensor.apply_state_payload(br#"{"state": "ON"}"#).unwrap();
        assert!(sensor.occupancy.occupancy);
    }

    #[test]
    fn parse_value_template_key_examples() {
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
}
