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
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoveryMessage {
    /// Human-readable name of the entity
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
}
