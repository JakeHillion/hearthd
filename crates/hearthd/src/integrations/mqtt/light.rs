use std::error::Error;

use crate::integrations::mqtt::discovery::DeviceInfo;
use crate::integrations::mqtt::discovery::DiscoveryMessage;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::Endpoint;
use crate::matter::EndpointId;
use crate::matter::LevelControlCluster;
use crate::matter::LevelControlCommand;
use crate::matter::Node;
use crate::matter::OnOffCluster;
use crate::matter::OnOffCommand;

/// Endpoint ID assigned to every Z2M-discovered device.
///
/// Zigbee2MQTT exposes single-function devices; we always model them as
/// endpoint 1 (the standard Matter root application endpoint).
pub const Z2M_ENDPOINT: EndpointId = 1;

/// MQTT-side Light entity.
///
/// Holds the Z2M metadata (topics, payloads, device info) plus the current
/// Matter cluster state for the OnOff and (optional) LevelControl clusters
/// that we expose. Everything that crosses the engine boundary uses the
/// Matter types.
#[derive(Debug, Clone)]
pub struct Light {
    pub entity_id: String,
    pub name: String,
    #[allow(dead_code)]
    pub unique_id: String,
    #[allow(dead_code)]
    pub device_info: Option<DeviceInfo>,

    pub state_topic: String,
    pub command_topic: String,

    pub on_off: OnOffCluster,
    pub level_control: Option<LevelControlCluster>,
}

impl Light {
    /// Create a Light entity from a Zigbee2MQTT discovery message
    pub fn from_discovery(
        discovery: DiscoveryMessage,
        entity_id: String,
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

        let level_control = if discovery.brightness.unwrap_or(false) {
            Some(LevelControlCluster::default())
        } else {
            None
        };

        Ok(Self {
            entity_id,
            name,
            unique_id,
            device_info: discovery.device,
            state_topic,
            command_topic,
            on_off: OnOffCluster::default(),
            level_control,
        })
    }

    /// True if this light exposes the LevelControl cluster.
    pub fn supports_brightness(&self) -> bool {
        self.level_control.is_some()
    }

    /// Build the Matter `Node` snapshot for this entity.
    pub fn to_node(&self, integration: &str, id: crate::matter::NodeId) -> Node {
        let mut endpoint = Endpoint::default();
        endpoint.clusters.insert(
            crate::matter::CLUSTER_NAME_ON_OFF.to_string(),
            Cluster::OnOff(self.on_off.clone()),
        );
        if let Some(lc) = &self.level_control {
            endpoint.clusters.insert(
                crate::matter::CLUSTER_NAME_LEVEL_CONTROL.to_string(),
                Cluster::LevelControl(lc.clone()),
            );
        }

        let mut endpoints = std::collections::HashMap::new();
        endpoints.insert(Z2M_ENDPOINT, endpoint);

        Node {
            id,
            entity_id: self.entity_id.clone(),
            integration: integration.to_string(),
            name: Some(self.name.clone()),
            endpoints,
        }
    }

    /// Apply an MQTT state-update payload to this light and return the
    /// list of clusters whose attributes changed, so the integration can
    /// emit one `AttributeChanged` message per cluster.
    ///
    /// Zigbee2MQTT sends state updates as JSON, e.g.
    /// `{"state": "ON", "brightness": 128}`. Both attributes ride on the
    /// same topic, so a single payload can touch both clusters.
    pub fn apply_state_payload(&mut self, payload: &[u8]) -> Result<Vec<Cluster>, Box<dyn Error>> {
        let json_str = std::str::from_utf8(payload)?;
        let state_update: serde_json::Value = serde_json::from_str(json_str)?;

        let mut changed = Vec::new();

        if let Some(state_str) = state_update.get("state").and_then(|v| v.as_str()) {
            let new_on = state_str == "ON";
            if new_on != self.on_off.on_off {
                self.on_off.on_off = new_on;
            }
            changed.push(Cluster::OnOff(self.on_off.clone()));
        }

        if let Some(lc) = self.level_control.as_mut() {
            if let Some(brightness) = state_update.get("brightness").and_then(|v| v.as_u64()) {
                let new_level = Some(brightness as u8);
                if new_level != lc.current_level {
                    lc.current_level = new_level;
                }
                changed.push(Cluster::LevelControl(lc.clone()));
            }
        }

        Ok(changed)
    }

    /// Translate a Matter cluster command into a Zigbee2MQTT JSON payload.
    ///
    /// Z2M co-locates on/off and brightness on a single set topic, so both
    /// `OnOff` and `LevelControl::MoveToLevel` produce a payload on the same
    /// `command_topic`. `OnOff::Toggle` uses the cached `on_off` state.
    pub fn command_payload(&self, command: &ClusterCommand) -> Result<Vec<u8>, Box<dyn Error>> {
        let payload = match command {
            ClusterCommand::OnOff(OnOffCommand::On) => serde_json::json!({ "state": "ON" }),
            ClusterCommand::OnOff(OnOffCommand::Off) => serde_json::json!({ "state": "OFF" }),
            ClusterCommand::OnOff(OnOffCommand::Toggle) => {
                let next = if self.on_off.on_off { "OFF" } else { "ON" };
                serde_json::json!({ "state": next })
            }
            ClusterCommand::LevelControl(LevelControlCommand::MoveToLevel { level, .. }) => {
                if !self.supports_brightness() {
                    return Err(
                        format!("Light {} does not expose LevelControl", self.entity_id).into(),
                    );
                }
                serde_json::json!({ "state": "ON", "brightness": level })
            }
        };

        Ok(serde_json::to_vec(&payload)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discovery_with_brightness(brightness: bool) -> DiscoveryMessage {
        DiscoveryMessage {
            name: Some("Test Light".to_string()),
            unique_id: Some("test_light".to_string()),
            state_topic: Some("zigbee2mqtt/light/state".to_string()),
            command_topic: Some("zigbee2mqtt/light/set".to_string()),
            brightness_state_topic: None,
            brightness_command_topic: None,
            device: None,
            payload_on: None,
            payload_off: None,
            brightness: Some(brightness),
            schema: None,
            device_class: None,
            value_template: None,
        }
    }

    #[test]
    fn light_with_brightness_has_level_control() {
        let light = Light::from_discovery(
            discovery_with_brightness(true),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        assert!(light.supports_brightness());
        assert_eq!(light.on_off, OnOffCluster::default());
        assert_eq!(light.level_control, Some(LevelControlCluster::default()));
    }

    #[test]
    fn light_without_brightness_omits_level_control() {
        let light = Light::from_discovery(
            discovery_with_brightness(false),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        assert!(!light.supports_brightness());
        assert!(light.level_control.is_none());
    }

    #[test]
    fn apply_state_payload_updates_both_clusters() {
        let mut light = Light::from_discovery(
            discovery_with_brightness(true),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();

        let changed = light
            .apply_state_payload(br#"{"state": "ON", "brightness": 128}"#)
            .unwrap();
        assert_eq!(changed.len(), 2);
        assert!(light.on_off.on_off);
        assert_eq!(
            light.level_control.as_ref().unwrap().current_level,
            Some(128)
        );
    }

    #[test]
    fn command_payload_for_move_to_level() {
        let light = Light::from_discovery(
            discovery_with_brightness(true),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        let payload = light
            .command_payload(&ClusterCommand::LevelControl(
                LevelControlCommand::MoveToLevel {
                    level: 200,
                    transition_time: None,
                },
            ))
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(json["state"], "ON");
        assert_eq!(json["brightness"], 200);
    }

    #[test]
    fn command_payload_for_toggle_uses_cached_state() {
        let mut light = Light::from_discovery(
            discovery_with_brightness(true),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        light.on_off.on_off = true;
        let payload = light
            .command_payload(&ClusterCommand::OnOff(OnOffCommand::Toggle))
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(json["state"], "OFF");
    }
}
