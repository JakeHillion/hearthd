use std::error::Error;

use crate::integrations::mqtt::discovery::DeviceInfo;
use crate::integrations::mqtt::discovery::DiscoveryMessage;
use crate::integrations::mqtt::discovery::parse_value_template_key;
use crate::integrations::mqtt::light::Z2M_ENDPOINT;
use crate::matter::Cluster;
use crate::matter::Endpoint;
use crate::matter::Node;
use crate::matter::TemperatureMeasurementCluster;

/// A numeric measurement channel on a Z2M sensor device.
///
/// Pairs the JSON key in the shared `value_json` state payload (e.g.
/// `"temperature"`) with the Matter cluster holding its current value.
#[derive(Debug, Clone)]
struct Channel<T> {
    key: String,
    cluster: T,
}

/// MQTT-side numeric sensor (temperature, ...).
///
/// A single physical Z2M device publishes each reading as its own `sensor`
/// discovery component, but they all share one state topic and one JSON
/// payload. hearthd models the device as a single Matter node whose
/// measurement clusters are populated from that shared payload.
#[derive(Debug, Clone)]
pub struct Sensor {
    pub entity_id: String,
    pub name: String,
    #[allow(dead_code)]
    pub unique_id: String,
    #[allow(dead_code)]
    pub device_info: Option<DeviceInfo>,

    pub state_topic: String,

    temperature: Option<Channel<TemperatureMeasurementCluster>>,
}

impl Sensor {
    /// Create a temperature `Sensor` from a Zigbee2MQTT `sensor` discovery.
    pub fn from_temperature_discovery(
        discovery: DiscoveryMessage,
        entity_id: String,
        node_id: String,
    ) -> Result<Self, Box<dyn Error>> {
        let unique_id = discovery
            .unique_id
            .unwrap_or_else(|| format!("{}_sensor", node_id));

        let name = discovery
            .name
            .unwrap_or_else(|| format!("Sensor {}", node_id));

        let state_topic = discovery
            .state_topic
            .ok_or("Missing state_topic in discovery message")?;

        // The `value_template` names the key in the shared JSON payload
        // (e.g. `{{ value_json.temperature }}`); default to the device class.
        let key = discovery
            .value_template
            .as_deref()
            .and_then(parse_value_template_key)
            .unwrap_or("temperature")
            .to_string();

        Ok(Self {
            entity_id,
            name,
            unique_id,
            device_info: discovery.device,
            state_topic,
            temperature: Some(Channel {
                key,
                cluster: TemperatureMeasurementCluster::default(),
            }),
        })
    }

    /// Build the Matter `Node` snapshot for this sensor.
    pub fn to_node(&self, integration: &str) -> Node {
        let mut endpoint = Endpoint::default();
        if let Some(temp) = &self.temperature {
            endpoint.clusters.insert(
                crate::matter::CLUSTER_NAME_TEMPERATURE_MEASUREMENT.to_string(),
                Cluster::TemperatureMeasurement(temp.cluster.clone()),
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

    /// Convert a Celsius reading to Matter's int16 `MeasuredValue`, expressed
    /// in hundredths of a degree and saturated to the representable range.
    fn celsius_to_measured_value(celsius: f64) -> i16 {
        (celsius * 100.0)
            .round()
            .clamp(i16::MIN as f64, i16::MAX as f64) as i16
    }

    /// Apply an MQTT state-update payload and return the list of clusters
    /// whose attributes changed, so the integration can emit one
    /// `AttributeChanged` message per cluster.
    ///
    /// Zigbee2MQTT sends the whole device state as one JSON object, e.g.
    /// `{"temperature": 22.5, "humidity": 55.3, "battery": 90}`; each channel
    /// reads its own key out of it.
    pub fn apply_state_payload(&mut self, payload: &[u8]) -> Result<Vec<Cluster>, Box<dyn Error>> {
        let json_str = std::str::from_utf8(payload)?;
        let state_update: serde_json::Value = serde_json::from_str(json_str)?;

        let mut changed = Vec::new();

        if let Some(temp) = self.temperature.as_mut() {
            if let Some(celsius) = state_update.get(&temp.key).and_then(|v| v.as_f64()) {
                temp.cluster.measured_value = Some(Self::celsius_to_measured_value(celsius));
                changed.push(Cluster::TemperatureMeasurement(temp.cluster.clone()));
            }
        }

        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temperature_discovery() -> DiscoveryMessage {
        DiscoveryMessage {
            name: Some("Living Room Temperature".to_string()),
            unique_id: Some("0x00124b001234abcd_temperature".to_string()),
            state_topic: Some("zigbee2mqtt/climate_sensor".to_string()),
            command_topic: None,
            brightness_state_topic: None,
            brightness_command_topic: None,
            device: None,
            payload_on: None,
            payload_off: None,
            brightness: None,
            schema: None,
            device_class: Some("temperature".to_string()),
            value_template: Some("{{ value_json.temperature }}".to_string()),
        }
    }

    #[test]
    fn from_discovery_sets_defaults() {
        let sensor = Sensor::from_temperature_discovery(
            temperature_discovery(),
            "sensor.climate".to_string(),
            "climate".to_string(),
        )
        .unwrap();

        assert_eq!(sensor.name, "Living Room Temperature");
        assert_eq!(sensor.state_topic, "zigbee2mqtt/climate_sensor");
        let node = sensor.to_node("mqtt");
        let endpoint = node.endpoints.get(&Z2M_ENDPOINT).unwrap();
        assert!(
            endpoint
                .clusters
                .contains_key(crate::matter::CLUSTER_NAME_TEMPERATURE_MEASUREMENT)
        );
    }

    #[test]
    fn from_discovery_rejects_missing_state_topic() {
        let mut discovery = temperature_discovery();
        discovery.state_topic = None;
        let result = Sensor::from_temperature_discovery(
            discovery,
            "sensor.test".to_string(),
            "test".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn apply_state_payload_scales_temperature() {
        let mut sensor = Sensor::from_temperature_discovery(
            temperature_discovery(),
            "sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        let changed = sensor
            .apply_state_payload(br#"{"temperature": 22.5, "humidity": 55.3, "battery": 90}"#)
            .unwrap();
        assert_eq!(changed.len(), 1);
        assert!(matches!(
            changed[0],
            Cluster::TemperatureMeasurement(ref c) if c.measured_value == Some(2250)
        ));
    }

    #[test]
    fn apply_state_payload_ignores_payload_without_temperature() {
        let mut sensor = Sensor::from_temperature_discovery(
            temperature_discovery(),
            "sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();

        let changed = sensor
            .apply_state_payload(br#"{"humidity": 55.3, "battery": 90}"#)
            .unwrap();
        assert!(changed.is_empty());
    }

    #[test]
    fn celsius_conversion_handles_negative_and_rounding() {
        assert_eq!(Sensor::celsius_to_measured_value(-5.0), -500);
        assert_eq!(Sensor::celsius_to_measured_value(21.345), 2135);
    }
}
