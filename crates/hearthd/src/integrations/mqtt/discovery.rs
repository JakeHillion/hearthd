use serde::{Deserialize, Serialize};

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
    pub payload_on: Option<String>,

    /// Payload to send when turning off
    pub payload_off: Option<String>,

    /// Whether brightness is supported
    pub brightness: Option<bool>,

    /// Schema type (default is "default")
    pub schema: Option<String>,
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

    /// Software version
    pub sw_version: Option<String>,

    /// Hardware version
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
}
