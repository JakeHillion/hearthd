use std::collections::HashSet;
use std::error::Error;

use crate::integrations::mqtt::discovery::DeviceInfo;
use crate::integrations::mqtt::discovery::DiscoveryMessage;
use crate::integrations::mqtt::discovery::entity_name;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::ColorControlCluster;
use crate::matter::ColorControlCommand;
use crate::matter::ColorMode;
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
    pub color_control: Option<ColorControlCluster>,

    /// Colour modes the device natively supports (e.g. "hs", "xy",
    /// "color_temp"). Used to reject commands in unsupported formats.
    supported_color_modes: HashSet<String>,
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
            .as_ref()
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("{}_light", node_id));

        let name = entity_name(&discovery, || format!("Light {}", node_id));

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

        let supported_color_modes: HashSet<String> = discovery
            .supported_color_modes
            .into_iter()
            .map(|s| s.to_lowercase())
            .filter(|s| is_color_mode(s))
            .collect();

        let color_control = if supported_color_modes.is_empty() {
            None
        } else {
            Some(ColorControlCluster {
                color_mode: discovery.color_mode.as_deref().and_then(parse_color_mode),
                ..ColorControlCluster::default()
            })
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
            color_control,
            supported_color_modes,
        })
    }

    /// True if this light exposes the LevelControl cluster.
    pub fn supports_brightness(&self) -> bool {
        self.level_control.is_some()
    }

    /// True if the given Z2M-style colour mode is supported by the device.
    fn supports_color_mode(&self, mode: &str) -> bool {
        self.supported_color_modes.contains(mode)
    }

    /// Build the Matter `Node` snapshot for this entity.
    pub fn to_node(&self, integration: &str) -> Node {
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
        if let Some(cc) = &self.color_control {
            endpoint.clusters.insert(
                crate::matter::CLUSTER_NAME_COLOR_CONTROL.to_string(),
                Cluster::ColorControl(cc.clone()),
            );
        }

        let mut endpoints = std::collections::HashMap::new();
        endpoints.insert(Z2M_ENDPOINT, endpoint);

        Node {
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
    /// `{"state": "ON", "brightness": 128}`. Multiple attributes ride on the
    /// same topic, so a single payload can touch several clusters.
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

        if let Some(cc) = self.color_control.as_mut() {
            let mut color_changed = false;

            if let Some(color) = state_update.get("color").and_then(|v| v.as_object()) {
                if let Some(hue) = color.get("h").and_then(|v| v.as_u64()) {
                    let new_hue = Some(hue as u8);
                    if new_hue != cc.current_hue {
                        cc.current_hue = new_hue;
                        color_changed = true;
                    }
                }
                if let Some(saturation) = color.get("s").and_then(|v| v.as_u64()) {
                    let new_saturation = Some(saturation as u8);
                    if new_saturation != cc.current_saturation {
                        cc.current_saturation = new_saturation;
                        color_changed = true;
                    }
                }
                if let Some(x) = color.get("x").and_then(|v| v.as_u64()) {
                    let new_x = Some(x as u16);
                    if new_x != cc.current_x {
                        cc.current_x = new_x;
                        color_changed = true;
                    }
                }
                if let Some(y) = color.get("y").and_then(|v| v.as_u64()) {
                    let new_y = Some(y as u16);
                    if new_y != cc.current_y {
                        cc.current_y = new_y;
                        color_changed = true;
                    }
                }
            }

            if let Some(temp) = state_update.get("color_temp").and_then(|v| v.as_u64()) {
                let new_temp = Some(temp as u16);
                if new_temp != cc.color_temperature_mireds {
                    cc.color_temperature_mireds = new_temp;
                    color_changed = true;
                }
            }

            if let Some(mode) = state_update
                .get("color_mode")
                .and_then(|v| v.as_str())
                .and_then(parse_color_mode)
            {
                if cc.color_mode != Some(mode) {
                    cc.color_mode = Some(mode);
                    color_changed = true;
                }
            }

            if color_changed {
                changed.push(Cluster::ColorControl(cc.clone()));
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
            ClusterCommand::ColorControl(ColorControlCommand::MoveToHue { .. }) => {
                return Err(format!(
                    "Light {} only supports hue+saturation commands; use MoveToHueAndSaturation",
                    self.entity_id
                )
                .into());
            }
            ClusterCommand::ColorControl(ColorControlCommand::MoveToSaturation { .. }) => {
                return Err(format!(
                    "Light {} only supports hue+saturation commands; use MoveToHueAndSaturation",
                    self.entity_id
                )
                .into());
            }
            ClusterCommand::ColorControl(ColorControlCommand::MoveToHueAndSaturation {
                hue,
                saturation,
                ..
            }) => {
                if !self.supports_color_mode("hs") {
                    return Err(format!(
                        "Light {} does not support hs colour mode",
                        self.entity_id
                    )
                    .into());
                }
                serde_json::json!({
                    "state": "ON",
                    "color": { "h": hue, "s": saturation }
                })
            }
            ClusterCommand::ColorControl(ColorControlCommand::MoveToColor { x, y, .. }) => {
                if !self.supports_color_mode("xy") {
                    return Err(format!(
                        "Light {} does not support xy colour mode",
                        self.entity_id
                    )
                    .into());
                }
                serde_json::json!({
                    "state": "ON",
                    "color": { "x": x, "y": y }
                })
            }
            ClusterCommand::ColorControl(ColorControlCommand::MoveToColorTemperature {
                color_temperature_mireds,
                ..
            }) => {
                if !self.supports_color_mode("color_temp") {
                    return Err(format!(
                        "Light {} does not support color_temp colour mode",
                        self.entity_id
                    )
                    .into());
                }
                serde_json::json!({
                    "state": "ON",
                    "color_temp": color_temperature_mireds
                })
            }
            other => {
                return Err(format!(
                    "Light {} does not expose cluster 0x{:04X}",
                    self.entity_id,
                    other.cluster_id()
                )
                .into());
            }
        };

        Ok(serde_json::to_vec(&payload)?)
    }
}

/// True if the given Z2M `supported_color_modes` entry is a real colour
/// capability rather than just brightness control.
fn is_color_mode(mode: &str) -> bool {
    matches!(mode, "hs" | "xy" | "color_temp")
}

/// Parse a Z2M-style colour mode string into a Matter `ColorMode`.
fn parse_color_mode(mode: &str) -> Option<ColorMode> {
    match mode {
        "hs" => Some(ColorMode::HueSaturation),
        "xy" => Some(ColorMode::Xy),
        "color_temp" => Some(ColorMode::ColorTemperature),
        _ => None,
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
            supported_color_modes: Vec::new(),
            color_mode: None,
            min_mireds: None,
            max_mireds: None,
            device_class: None,
            value_template: None,
        }
    }

    fn discovery_with_color_modes(modes: &[&str]) -> DiscoveryMessage {
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
            brightness: Some(true),
            schema: None,
            supported_color_modes: modes.iter().map(|s| s.to_string()).collect(),
            color_mode: None,
            min_mireds: None,
            max_mireds: None,
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

    #[test]
    fn light_with_color_modes_has_color_control() {
        let light = Light::from_discovery(
            discovery_with_color_modes(&["hs", "color_temp"]),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        assert!(light.color_control.is_some());
    }

    #[test]
    fn light_without_color_modes_omits_color_control() {
        let light = Light::from_discovery(
            discovery_with_brightness(true),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        assert!(light.color_control.is_none());
    }

    #[test]
    fn apply_state_payload_updates_color_control_hs_and_temp() {
        let mut light = Light::from_discovery(
            discovery_with_color_modes(&["hs", "color_temp"]),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();

        let changed = light
            .apply_state_payload(
                br#"{"state": "ON", "brightness": 200, "color": {"h": 120, "s": 80}, "color_temp": 250, "color_mode": "hs"}"#,
            )
            .unwrap();

        let cluster_names: Vec<&str> = changed.iter().map(|c| c.name()).collect();
        assert!(cluster_names.contains(&"OnOff"));
        assert!(cluster_names.contains(&"LevelControl"));
        assert!(cluster_names.contains(&"ColorControl"));

        let cc = light.color_control.as_ref().unwrap();
        assert_eq!(cc.current_hue, Some(120));
        assert_eq!(cc.current_saturation, Some(80));
        assert_eq!(cc.color_temperature_mireds, Some(250));
        assert_eq!(cc.color_mode, Some(ColorMode::HueSaturation));
    }

    #[test]
    fn apply_state_payload_updates_color_control_xy() {
        let mut light = Light::from_discovery(
            discovery_with_color_modes(&["xy"]),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();

        let changed = light
            .apply_state_payload(br#"{"color": {"x": 30000, "y": 15000}, "color_mode": "xy"}"#)
            .unwrap();

        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].name(), "ColorControl");

        let cc = light.color_control.as_ref().unwrap();
        assert_eq!(cc.current_x, Some(30000));
        assert_eq!(cc.current_y, Some(15000));
        assert_eq!(cc.color_mode, Some(ColorMode::Xy));
    }

    #[test]
    fn command_payload_for_move_to_hue_and_saturation() {
        let light = Light::from_discovery(
            discovery_with_color_modes(&["hs"]),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        let payload = light
            .command_payload(&ClusterCommand::ColorControl(
                ColorControlCommand::MoveToHueAndSaturation {
                    hue: 60,
                    saturation: 200,
                    transition_time: None,
                },
            ))
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(json["state"], "ON");
        assert_eq!(json["color"]["h"], 60);
        assert_eq!(json["color"]["s"], 200);
    }

    #[test]
    fn command_payload_for_move_to_color_xy() {
        let light = Light::from_discovery(
            discovery_with_color_modes(&["xy"]),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        let payload = light
            .command_payload(&ClusterCommand::ColorControl(
                ColorControlCommand::MoveToColor {
                    x: 30000,
                    y: 15000,
                    transition_time: None,
                },
            ))
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(json["state"], "ON");
        assert_eq!(json["color"]["x"], 30000);
        assert_eq!(json["color"]["y"], 15000);
    }

    #[test]
    fn command_payload_for_move_to_color_temperature() {
        let light = Light::from_discovery(
            discovery_with_color_modes(&["color_temp"]),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        let payload = light
            .command_payload(&ClusterCommand::ColorControl(
                ColorControlCommand::MoveToColorTemperature {
                    color_temperature_mireds: 300,
                    transition_time: None,
                },
            ))
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(json["state"], "ON");
        assert_eq!(json["color_temp"], 300);
    }

    #[test]
    fn unsupported_color_command_returns_error() {
        let light = Light::from_discovery(
            discovery_with_color_modes(&["hs"]),
            "light.test".to_string(),
            "test_node".to_string(),
        )
        .unwrap();
        let result = light.command_payload(&ClusterCommand::ColorControl(
            ColorControlCommand::MoveToColor {
                x: 30000,
                y: 15000,
                transition_time: None,
            },
        ));
        assert!(result.is_err());
    }
}
