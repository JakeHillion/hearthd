use std::error::Error;
use std::fmt;

use serde::Deserialize;
use serde::Serialize;

use crate::integrations::mqtt::discovery::DeviceInfo;
use crate::integrations::mqtt::discovery::DiscoveryMessage;
use crate::integrations::mqtt::discovery::entity_name;
use crate::integrations::mqtt::discovery::parse_value_template_key;
use crate::integrations::mqtt::light::Z2M_ENDPOINT;
use crate::matter::BooleanStateCluster;
use crate::matter::Cluster;
use crate::matter::DeviceType;
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

/// Which Matter sensor a binary sensor is, chosen from its device class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinarySensorKind {
    /// Motion, occupancy and presence: an Occupancy Sensor carrying
    /// Occupancy Sensing.
    Occupancy,
    /// Doors, windows, openings and garage doors: a Contact Sensor carrying
    /// Boolean State, whose value is true when the contact is closed, which
    /// is also how Zigbee2MQTT reports `contact`.
    Contact,
}

impl BinarySensorKind {
    /// The kind a device class maps to, or `None` for classes hearthd has no
    /// cluster for.
    pub fn for_class(class: &BinarySensorDeviceClass) -> Option<Self> {
        match class {
            BinarySensorDeviceClass::Motion
            | BinarySensorDeviceClass::Occupancy
            | BinarySensorDeviceClass::Presence => Some(Self::Occupancy),
            BinarySensorDeviceClass::Door
            | BinarySensorDeviceClass::Window
            | BinarySensorDeviceClass::Opening
            | BinarySensorDeviceClass::GarageDoor => Some(Self::Contact),
            _ => None,
        }
    }

    fn device_type(self) -> DeviceType {
        match self {
            Self::Occupancy => DeviceType::OccupancySensor,
            Self::Contact => DeviceType::ContactSensor,
        }
    }
}

/// MQTT-side binary sensor (motion, occupancy, door, etc.).
///
/// Holds Z2M metadata plus the current boolean, rendered as the cluster its
/// kind calls for.
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

    pub kind: BinarySensorKind,
    pub active: bool,
}

impl BinarySensor {
    pub fn from_discovery(
        discovery: DiscoveryMessage,
        kind: BinarySensorKind,
        entity_id: String,
        node_id: String,
    ) -> Result<Self, Box<dyn Error>> {
        let unique_id = discovery
            .unique_id
            .as_ref()
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("{}_binary_sensor", node_id));

        let name = entity_name(&discovery, || format!("Binary Sensor {}", node_id));

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
            kind,
            active: false,
        })
    }

    /// The current value as the cluster this sensor kind carries.
    pub fn cluster(&self) -> Cluster {
        match self.kind {
            BinarySensorKind::Occupancy => Cluster::OccupancySensing(OccupancySensingCluster {
                occupancy: self.active,
            }),
            BinarySensorKind::Contact => Cluster::BooleanState(BooleanStateCluster {
                state_value: self.active,
            }),
        }
    }

    /// Build the Matter `Node` snapshot for this sensor.
    pub fn to_node(&self, integration: &str) -> Node {
        let endpoint =
            Endpoint::from_clusters([self.cluster()]).with_device_types([self.kind.device_type()]);

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
            self.active = match value {
                serde_json::Value::Bool(b) => *b,
                serde_json::Value::String(s) => s == "ON" || s == "true",
                _ => false,
            };
            return Ok(Some(self.cluster()));
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
            supported_color_modes: Vec::new(),
            color_mode: None,
            min_mireds: None,
            max_mireds: None,
            device_class: Some("motion".to_string()),
            value_template: Some("{{ value_json.occupancy }}".to_string()),
        }
    }

    #[test]
    fn from_discovery_sets_defaults() {
        let sensor = BinarySensor::from_discovery(
            motion_discovery(),
            BinarySensorKind::Occupancy,
            "binary_sensor.living_room".to_string(),
            "living_room".to_string(),
        )
        .unwrap();

        assert_eq!(sensor.name, "Living Room Motion");
        assert_eq!(sensor.device_class, Some(BinarySensorDeviceClass::Motion));
        assert_eq!(sensor.state_topic, "zigbee2mqtt/motion_sensor");
        assert!(!sensor.active);
    }

    #[test]
    fn from_discovery_rejects_missing_state_topic() {
        let mut discovery = motion_discovery();
        discovery.state_topic = None;
        let result = BinarySensor::from_discovery(
            discovery,
            BinarySensorKind::Occupancy,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn apply_state_payload_updates_occupancy() {
        let mut sensor = BinarySensor::from_discovery(
            motion_discovery(),
            BinarySensorKind::Occupancy,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        let changed = sensor
            .apply_state_payload(br#"{"occupancy": true, "battery": 95}"#)
            .unwrap();
        assert!(matches!(changed, Some(Cluster::OccupancySensing(_))));
        assert!(sensor.active);

        let changed = sensor
            .apply_state_payload(br#"{"occupancy": false}"#)
            .unwrap();
        assert!(matches!(changed, Some(Cluster::OccupancySensing(_))));
        assert!(!sensor.active);
    }

    #[test]
    fn apply_state_payload_falls_back_to_state_key() {
        let mut discovery = motion_discovery();
        discovery.value_template = None;
        let mut sensor = BinarySensor::from_discovery(
            discovery,
            BinarySensorKind::Occupancy,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        sensor.apply_state_payload(br#"{"state": "ON"}"#).unwrap();
        assert!(sensor.active);
    }

    #[test]
    fn device_classes_map_to_a_kind_or_nothing() {
        use BinarySensorDeviceClass as C;
        for class in [C::Motion, C::Occupancy, C::Presence] {
            assert_eq!(
                BinarySensorKind::for_class(&class),
                Some(BinarySensorKind::Occupancy)
            );
        }
        for class in [C::Door, C::Window, C::Opening, C::GarageDoor] {
            assert_eq!(
                BinarySensorKind::for_class(&class),
                Some(BinarySensorKind::Contact)
            );
        }
        assert_eq!(BinarySensorKind::for_class(&C::Vibration), None);
        assert_eq!(BinarySensorKind::for_class(&C::Unknown("x".into())), None);
    }

    #[test]
    fn a_motion_sensor_is_a_conformant_occupancy_sensor() {
        let sensor = BinarySensor::from_discovery(
            motion_discovery(),
            BinarySensorKind::Occupancy,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();
        let endpoint = sensor.to_node("mqtt").endpoints[&Z2M_ENDPOINT].clone();
        assert_eq!(endpoint.device_types, [DeviceType::OccupancySensor]);
        assert_eq!(endpoint.missing_mandatory_clusters(), []);
    }

    #[test]
    fn a_door_sensor_is_a_contact_sensor_carrying_boolean_state() {
        let mut discovery = motion_discovery();
        discovery.device_class = Some("door".to_string());
        discovery.value_template = Some("{{ value_json.contact }}".to_string());
        let mut sensor = BinarySensor::from_discovery(
            discovery,
            BinarySensorKind::Contact,
            "binary_sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        let endpoint = sensor.to_node("mqtt").endpoints[&Z2M_ENDPOINT].clone();
        assert_eq!(endpoint.device_types, [DeviceType::ContactSensor]);
        assert_eq!(endpoint.missing_mandatory_clusters(), []);
        assert!(!endpoint.clusters.contains_key("OccupancySensing"));

        // Zigbee2MQTT reports `contact: true` for a closed door, which is
        // what Boolean State means by true on a contact sensor.
        let changed = sensor.apply_state_payload(br#"{"contact": true}"#).unwrap();
        assert_eq!(
            changed,
            Some(Cluster::BooleanState(BooleanStateCluster {
                state_value: true
            }))
        );
    }
}
