use serde::Deserialize;
use serde::Serialize;

/// Deserialize a field that can be either a string or an integer.
///
/// Zigbee2MQTT sends version fields like `hw_version` as integers, but the
/// Home Assistant discovery schema defines them as strings. This helper
/// accepts both types and converts integers to strings.
fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrInt;

    impl<'de> de::Visitor<'de> for StringOrInt {
        type Value = Option<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("string, integer, or null")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
    }

    deserializer.deserialize_any(StringOrInt)
}

/// Deserialize a field that can be a string, boolean, or integer.
///
/// Zigbee2MQTT sends `payload_on`/`payload_off` as `"ON"`/`"OFF"` for lights
/// but `true`/`false` for binary sensors. This helper accepts any scalar type
/// and converts to a string.
fn deserialize_string_or_scalar<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrScalar;

    impl<'de> de::Visitor<'de> for StringOrScalar {
        type Value = Option<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("string, boolean, integer, or null")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
    }

    deserializer.deserialize_any(StringOrScalar)
}

/// Discovery message for Zigbee2MQTT devices
///
/// This struct represents the JSON payload sent by Zigbee2MQTT on discovery topics.
/// Based on Home Assistant's MQTT discovery protocol.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DiscoveryMessage {
    /// Human-readable name of the entity
    #[serde(default)]
    pub name: Option<String>,

    /// Unique identifier for this entity
    pub unique_id: Option<String>,

    /// Topic to receive state updates
    pub state_topic: Option<String>,

    /// Topic to send commands
    pub command_topic: Option<String>,

    /// Topic to receive brightness state (for lights)
    pub brightness_state_topic: Option<String>,

    /// Topic to send brightness commands (for lights)
    pub brightness_command_topic: Option<String>,

    /// Device information
    pub device: Option<DeviceInfo>,

    /// Payload to send when turning on
    #[serde(default, deserialize_with = "deserialize_string_or_scalar")]
    pub payload_on: Option<String>,

    /// Payload to send when turning off
    #[serde(default, deserialize_with = "deserialize_string_or_scalar")]
    pub payload_off: Option<String>,

    /// Whether brightness is supported
    pub brightness: Option<bool>,

    /// Schema type (default is "default")
    pub schema: Option<String>,

    /// Supported color modes for lights (e.g. "hs", "xy", "color_temp").
    #[serde(default)]
    pub supported_color_modes: Vec<String>,

    /// Active color mode reported by a light ("hs", "xy", or "color_temp").
    pub color_mode: Option<String>,

    /// Minimum color temperature in mireds.
    pub min_mireds: Option<u16>,

    /// Maximum color temperature in mireds.
    pub max_mireds: Option<u16>,

    /// Device class (e.g., "motion", "door", "window") for binary sensors
    pub device_class: Option<String>,

    /// Value template for extracting state from JSON payload
    /// e.g., "{{ value_json.occupancy }}"
    pub value_template: Option<String>,
}

/// Device information from Zigbee2MQTT discovery
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeviceInfo {
    /// List of identifiers for this device
    pub identifiers: Vec<String>,

    /// Device name
    pub name: String,

    /// Manufacturer name
    pub manufacturer: Option<String>,

    /// Model name
    pub model: Option<String>,

    /// Software version (can be string or integer in Zigbee2MQTT)
    #[serde(default, deserialize_with = "deserialize_string_or_int")]
    pub sw_version: Option<String>,

    /// Hardware version (can be string or integer in Zigbee2MQTT)
    #[serde(default, deserialize_with = "deserialize_string_or_int")]
    pub hw_version: Option<String>,
}

/// Choose the best human-readable name for an entity from a discovery message.
///
/// Zigbee2MQTT usually leaves the per-component `name` field empty and puts
/// the friendly device name in `device.name`. Fall back to the provided
/// fallback only when neither source is available.
pub fn entity_name(discovery: &DiscoveryMessage, fallback: impl FnOnce() -> String) -> String {
    discovery
        .name
        .as_ref()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .map(|n| n.to_string())
        .or_else(|| discovery.device.as_ref().map(|d| d.name.clone()))
        .unwrap_or_else(fallback)
}

/// Extract the JSON key name from a Zigbee2MQTT value template.
///
/// Parses templates like `{{ value_json.occupancy }}` and returns `"occupancy"`.
/// Returns `None` if the template doesn't match the expected format.
pub fn parse_value_template_key(template: &str) -> Option<&str> {
    let inner = template
        .trim()
        .strip_prefix("{{")?
        .strip_suffix("}}")?
        .trim();
    inner.strip_prefix("value_json.")
}

/// Parse a discovery topic to extract component type, node_id, and object_id
///
/// Topic format: {prefix}/{component}/{node_id}/{object_id}/config
/// Example: homeassistant/light/0x00124b001234abcd/light/config
///
/// Returns: (component, node_id, object_id)
pub fn parse_discovery_topic(topic: &str, prefix: &str) -> Option<(String, String, String)> {
    // Remove the discovery prefix
    let without_prefix = topic.strip_prefix(prefix)?.strip_prefix('/')?;

    // Split the remaining parts
    let parts: Vec<&str> = without_prefix.split('/').collect();

    // We expect at least 4 parts: component/node_id/object_id/config
    if parts.len() < 4 || parts.last() != Some(&"config") {
        return None;
    }

    let component = parts[0].to_string();
    let node_id = parts[1].to_string();
    let object_id = parts[2].to_string();

    Some((component, node_id, object_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_discovery_topic() {
        let topic = "homeassistant/light/0x00124b001234abcd/light/config";
        let result = parse_discovery_topic(topic, "homeassistant");
        assert_eq!(
            result,
            Some((
                "light".to_string(),
                "0x00124b001234abcd".to_string(),
                "light".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_discovery_topic_invalid() {
        let topic = "homeassistant/light/0x00124b001234abcd";
        let result = parse_discovery_topic(topic, "homeassistant");
        assert_eq!(result, None);
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
        assert_eq!(
            parse_value_template_key("{{ value_json.temperature }}"),
            Some("temperature")
        );
        assert_eq!(parse_value_template_key("invalid"), None);
        assert_eq!(parse_value_template_key("{{ something_else }}"), None);
    }

    #[test]
    fn test_parse_binary_sensor_discovery_topic() {
        let topic = "homeassistant/binary_sensor/0x00124b001234abcd/occupancy/config";
        let result = parse_discovery_topic(topic, "homeassistant");
        assert_eq!(
            result,
            Some((
                "binary_sensor".to_string(),
                "0x00124b001234abcd".to_string(),
                "occupancy".to_string()
            ))
        );
    }

    #[test]
    fn entity_name_prefers_component_name() {
        let discovery = DiscoveryMessage {
            name: Some("Component Name".to_string()),
            device: Some(DeviceInfo {
                name: "Device Name".to_string(),
                identifiers: vec!["id".to_string()],
                manufacturer: None,
                model: None,
                sw_version: None,
                hw_version: None,
            }),
            ..DiscoveryMessage::default()
        };
        assert_eq!(
            entity_name(&discovery, || "fallback".to_string()),
            "Component Name"
        );
    }

    #[test]
    fn entity_name_falls_back_to_device_name() {
        let discovery = DiscoveryMessage {
            name: None,
            device: Some(DeviceInfo {
                name: "Device Name".to_string(),
                identifiers: vec!["id".to_string()],
                manufacturer: None,
                model: None,
                sw_version: None,
                hw_version: None,
            }),
            ..DiscoveryMessage::default()
        };
        assert_eq!(
            entity_name(&discovery, || "fallback".to_string()),
            "Device Name"
        );
    }

    #[test]
    fn entity_name_ignores_empty_component_name() {
        let discovery = DiscoveryMessage {
            name: Some("".to_string()),
            device: Some(DeviceInfo {
                name: "Device Name".to_string(),
                identifiers: vec!["id".to_string()],
                manufacturer: None,
                model: None,
                sw_version: None,
                hw_version: None,
            }),
            ..DiscoveryMessage::default()
        };
        assert_eq!(
            entity_name(&discovery, || "fallback".to_string()),
            "Device Name"
        );
    }

    #[test]
    fn entity_name_uses_fallback_when_no_name_available() {
        let discovery = DiscoveryMessage {
            name: None,
            device: None,
            ..DiscoveryMessage::default()
        };
        assert_eq!(
            entity_name(&discovery, || "fallback".to_string()),
            "fallback"
        );
    }
}
