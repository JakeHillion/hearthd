use std::error::Error;

use crate::engine::state::LightState;
use crate::integrations::mqtt::discovery::DeviceInfo;
use crate::integrations::mqtt::discovery::DiscoveryMessage;

/// Light entity
#[derive(Debug, Clone)]
pub struct Light {
    /// Entity ID (e.g., "light.living_room")
    #[allow(dead_code)]
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Unique identifier from Zigbee2MQTT
    #[allow(dead_code)]
    pub unique_id: String,

    /// Current state of the light
    pub state: LightState,

    /// Device information
    #[allow(dead_code)]
    pub device_info: Option<DeviceInfo>,

    /// Topic to receive state updates
    pub state_topic: String,

    /// Topic to send commands
    pub command_topic: String,

    /// Topic to receive brightness state
    #[allow(dead_code)]
    pub brightness_state_topic: Option<String>,

    /// Topic to send brightness commands
    #[allow(dead_code)]
    pub brightness_command_topic: Option<String>,

    /// Payloads for on/off commands
    #[allow(dead_code)]
    pub payload_on: String,
    #[allow(dead_code)]
    pub payload_off: String,

    /// Whether brightness is supported
    pub supports_brightness: bool,
}

impl Light {
    /// Create a Light entity from a Zigbee2MQTT discovery message
    pub fn from_discovery(
        discovery: DiscoveryMessage,
        id: String,
        node_id: String,
    ) -> Result<Self, Box<dyn Error>> {
        let unique_id = discovery
            .unique_id
            .unwrap_or_else(|| format!("{}_light", node_id));

        let name = discovery
            .name
            .unwrap_or_else(|| format!("Light {}", node_id));

        let state_topic = discovery
            .state_topic
            .ok_or("Missing state_topic in discovery message")?;

        let command_topic = discovery
            .command_topic
            .ok_or("Missing command_topic in discovery message")?;

        let supports_brightness = discovery.brightness.unwrap_or(false);

        Ok(Self {
            id,
            name,
            unique_id,
            state: LightState::default(),
            device_info: discovery.device,
            state_topic,
            command_topic,
            brightness_state_topic: discovery.brightness_state_topic,
            brightness_command_topic: discovery.brightness_command_topic,
            payload_on: discovery.payload_on.unwrap_or_else(|| "ON".to_string()),
            payload_off: discovery.payload_off.unwrap_or_else(|| "OFF".to_string()),
            supports_brightness,
        })
    }

    /// Update the light state from an MQTT payload
    ///
    /// Zigbee2MQTT sends state updates as JSON, e.g.:
    /// {"state": "ON", "brightness": 128}
    pub fn update_state(&mut self, payload: &[u8]) -> Result<(), Box<dyn Error>> {
        let json_str = std::str::from_utf8(payload)?;
        let state_update: serde_json::Value = serde_json::from_str(json_str)?;

        // Update on/off state
        if let Some(state_str) = state_update.get("state").and_then(|v| v.as_str()) {
            self.state.on = state_str == "ON";
        }

        // Update brightness if present and supported
        if self.supports_brightness {
            if let Some(brightness) = state_update.get("brightness").and_then(|v| v.as_u64()) {
                self.state.brightness = Some(brightness as u8);
            }
        }

        Ok(())
    }

    /// Generate a command payload to set the light state
    pub fn command_payload(&self, state: &LightState) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut payload = serde_json::json!({
            "state": if state.on { "ON" } else { "OFF" }
        });

        if self.supports_brightness {
            if let Some(brightness) = state.brightness {
                payload["brightness"] = serde_json::json!(brightness);
            }
        }

        Ok(serde_json::to_vec(&payload)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_light_state_default() {
        let state = LightState::default();
        assert!(!state.on);
        assert_eq!(state.brightness, None);
    }

    #[test]
    fn test_update_state() {
        let discovery = DiscoveryMessage {
            name: Some("Test Light".to_string()),
            unique_id: Some("test_light".to_string()),
            state_topic: Some("zigbee2mqtt/light/state".to_string()),
            command_topic: Some("zigbee2mqtt/light/set".to_string()),
            brightness_state_topic: None,
            brightness_command_topic: None,
            device: None,
            payload_on: None,
            payload_off: None,
            brightness: Some(true),
            schema: None,
            device_class: None,
            value_template: None,
        };

        let mut light =
            Light::from_discovery(discovery, "light.test".to_string(), "test_node".to_string())
                .unwrap();

        let payload = br#"{"state": "ON", "brightness": 128}"#;
        light.update_state(payload).unwrap();

        assert!(light.state.on);
        assert_eq!(light.state.brightness, Some(128));
    }
}
