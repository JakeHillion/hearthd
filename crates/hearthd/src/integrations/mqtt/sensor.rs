use std::error::Error;

use crate::integrations::mqtt::discovery::DeviceInfo;
use crate::integrations::mqtt::discovery::DiscoveryMessage;
use crate::integrations::mqtt::discovery::parse_value_template_key;
use crate::integrations::mqtt::light::Z2M_ENDPOINT;
use crate::matter::Cluster;
use crate::matter::Endpoint;
use crate::matter::Node;
use crate::matter::RelativeHumidityMeasurementCluster;
use crate::matter::TemperatureMeasurementCluster;

/// A numeric reading a Z2M `sensor` component can expose, mapped to a Matter
/// measurement cluster.
///
/// A physical device publishes each reading as its own `sensor` discovery
/// message (all sharing one state topic), so discovery resolves the
/// `device_class` to one of these and folds it onto the device's node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measurement {
    Temperature,
    Humidity,
}

impl Measurement {
    /// Resolve a Z2M `device_class` to a supported measurement, if any.
    pub fn from_device_class(device_class: Option<&str>) -> Option<Self> {
        match device_class {
            Some("temperature") => Some(Self::Temperature),
            Some("humidity") => Some(Self::Humidity),
            _ => None,
        }
    }

    /// Fallback key in the shared JSON payload when no `value_template` is set.
    fn default_key(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Humidity => "humidity",
        }
    }
}

/// A numeric measurement channel on a Z2M sensor device.
///
/// Pairs the JSON key in the shared `value_json` state payload (e.g.
/// `"temperature"`) with the Matter cluster holding its current value.
#[derive(Debug, Clone)]
struct Channel<T> {
    key: String,
    cluster: T,
}

/// MQTT-side numeric sensor (temperature, humidity, ...).
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
    humidity: Option<Channel<RelativeHumidityMeasurementCluster>>,
}

impl Sensor {
    /// Create a `Sensor` from a Zigbee2MQTT `sensor` discovery carrying the
    /// given `measurement`.
    pub fn from_discovery(
        discovery: DiscoveryMessage,
        measurement: Measurement,
        entity_id: String,
        node_id: String,
    ) -> Result<Self, Box<dyn Error>> {
        let unique_id = discovery
            .unique_id
            .clone()
            .unwrap_or_else(|| format!("{}_sensor", node_id));

        let name = discovery
            .name
            .clone()
            .unwrap_or_else(|| format!("Sensor {}", node_id));

        let state_topic = discovery
            .state_topic
            .clone()
            .ok_or("Missing state_topic in discovery message")?;

        let mut sensor = Self {
            entity_id,
            name,
            unique_id,
            device_info: discovery.device.clone(),
            state_topic,
            temperature: None,
            humidity: None,
        };
        sensor.set_channel(measurement, &discovery);
        Ok(sensor)
    }

    /// Fold an additional measurement onto an existing device node.
    ///
    /// Returns `true` if a new channel was added, or `false` if this
    /// measurement was already present (a Z2M re-discovery).
    pub fn add_channel(&mut self, measurement: Measurement, discovery: &DiscoveryMessage) -> bool {
        let already_present = match measurement {
            Measurement::Temperature => self.temperature.is_some(),
            Measurement::Humidity => self.humidity.is_some(),
        };
        if already_present {
            return false;
        }
        self.set_channel(measurement, discovery);
        true
    }

    /// Populate the channel for `measurement` from its discovery message.
    ///
    /// The `value_template` names the key in the shared JSON payload (e.g.
    /// `{{ value_json.temperature }}`); default to the measurement's key.
    fn set_channel(&mut self, measurement: Measurement, discovery: &DiscoveryMessage) {
        let key = discovery
            .value_template
            .as_deref()
            .and_then(parse_value_template_key)
            .unwrap_or_else(|| measurement.default_key())
            .to_string();

        match measurement {
            Measurement::Temperature => {
                self.temperature = Some(Channel {
                    key,
                    cluster: TemperatureMeasurementCluster::default(),
                });
            }
            Measurement::Humidity => {
                self.humidity = Some(Channel {
                    key,
                    cluster: RelativeHumidityMeasurementCluster::default(),
                });
            }
        }
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
        if let Some(humidity) = &self.humidity {
            endpoint.clusters.insert(
                crate::matter::CLUSTER_NAME_RELATIVE_HUMIDITY_MEASUREMENT.to_string(),
                Cluster::RelativeHumidityMeasurement(humidity.cluster.clone()),
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

    /// Convert a relative-humidity percentage to Matter's uint16
    /// `MeasuredValue`, expressed in hundredths of a percent and saturated to
    /// the representable range.
    fn percent_to_measured_value(percent: f64) -> u16 {
        (percent * 100.0).round().clamp(0.0, u16::MAX as f64) as u16
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

        if let Some(humidity) = self.humidity.as_mut() {
            if let Some(percent) = state_update.get(&humidity.key).and_then(|v| v.as_f64()) {
                humidity.cluster.measured_value = Some(Self::percent_to_measured_value(percent));
                changed.push(Cluster::RelativeHumidityMeasurement(
                    humidity.cluster.clone(),
                ));
            }
        }

        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discovery(device_class: &str, template: &str) -> DiscoveryMessage {
        DiscoveryMessage {
            name: Some(format!("Living Room {}", device_class)),
            unique_id: Some(format!("0x00124b001234abcd_{}", device_class)),
            state_topic: Some("zigbee2mqtt/climate_sensor".to_string()),
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
            device_class: Some(device_class.to_string()),
            value_template: Some(template.to_string()),
        }
    }

    fn temperature_discovery() -> DiscoveryMessage {
        discovery("temperature", "{{ value_json.temperature }}")
    }

    fn humidity_discovery() -> DiscoveryMessage {
        discovery("humidity", "{{ value_json.humidity }}")
    }

    #[test]
    fn measurement_from_device_class() {
        assert_eq!(
            Measurement::from_device_class(Some("temperature")),
            Some(Measurement::Temperature)
        );
        assert_eq!(
            Measurement::from_device_class(Some("humidity")),
            Some(Measurement::Humidity)
        );
        assert_eq!(Measurement::from_device_class(Some("battery")), None);
        assert_eq!(Measurement::from_device_class(None), None);
    }

    #[test]
    fn from_discovery_sets_defaults() {
        let sensor = Sensor::from_discovery(
            temperature_discovery(),
            Measurement::Temperature,
            "sensor.climate".to_string(),
            "climate".to_string(),
        )
        .unwrap();

        assert_eq!(sensor.name, "Living Room temperature");
        assert_eq!(sensor.state_topic, "zigbee2mqtt/climate_sensor");
        let node = sensor.to_node("mqtt");
        let endpoint = node.endpoints.get(&Z2M_ENDPOINT).unwrap();
        assert!(
            endpoint
                .clusters
                .contains_key(crate::matter::CLUSTER_NAME_TEMPERATURE_MEASUREMENT)
        );
        assert!(
            !endpoint
                .clusters
                .contains_key(crate::matter::CLUSTER_NAME_RELATIVE_HUMIDITY_MEASUREMENT)
        );
    }

    #[test]
    fn from_discovery_rejects_missing_state_topic() {
        let mut discovery = temperature_discovery();
        discovery.state_topic = None;
        let result = Sensor::from_discovery(
            discovery,
            Measurement::Temperature,
            "sensor.test".to_string(),
            "test".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn add_channel_merges_humidity_onto_temperature() {
        let mut sensor = Sensor::from_discovery(
            temperature_discovery(),
            Measurement::Temperature,
            "sensor.climate".to_string(),
            "climate".to_string(),
        )
        .unwrap();

        assert!(sensor.add_channel(Measurement::Humidity, &humidity_discovery()));
        // A repeat discovery for the same measurement is a no-op.
        assert!(!sensor.add_channel(Measurement::Humidity, &humidity_discovery()));

        let node = sensor.to_node("mqtt");
        let endpoint = node.endpoints.get(&Z2M_ENDPOINT).unwrap();
        assert!(
            endpoint
                .clusters
                .contains_key(crate::matter::CLUSTER_NAME_TEMPERATURE_MEASUREMENT)
        );
        assert!(
            endpoint
                .clusters
                .contains_key(crate::matter::CLUSTER_NAME_RELATIVE_HUMIDITY_MEASUREMENT)
        );
    }

    #[test]
    fn apply_state_payload_scales_both_channels() {
        let mut sensor = Sensor::from_discovery(
            temperature_discovery(),
            Measurement::Temperature,
            "sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();
        sensor.add_channel(Measurement::Humidity, &humidity_discovery());

        let changed = sensor
            .apply_state_payload(br#"{"temperature": 22.5, "humidity": 55.3, "battery": 90}"#)
            .unwrap();
        assert_eq!(changed.len(), 2);
        assert!(changed.iter().any(|c| matches!(
            c,
            Cluster::TemperatureMeasurement(t) if t.measured_value == Some(2250)
        )));
        assert!(changed.iter().any(|c| matches!(
            c,
            Cluster::RelativeHumidityMeasurement(h) if h.measured_value == Some(5530)
        )));
    }

    #[test]
    fn apply_state_payload_reports_only_present_keys() {
        let mut sensor = Sensor::from_discovery(
            temperature_discovery(),
            Measurement::Temperature,
            "sensor.test".to_string(),
            "test".to_string(),
        )
        .unwrap();
        sensor.add_channel(Measurement::Humidity, &humidity_discovery());

        let changed = sensor
            .apply_state_payload(br#"{"humidity": 55.3, "battery": 90}"#)
            .unwrap();
        assert_eq!(changed.len(), 1);
        assert!(matches!(
            changed[0],
            Cluster::RelativeHumidityMeasurement(ref h) if h.measured_value == Some(5530)
        ));
    }

    #[test]
    fn measured_value_conversions() {
        assert_eq!(Sensor::celsius_to_measured_value(-5.0), -500);
        assert_eq!(Sensor::celsius_to_measured_value(21.345), 2135);
        assert_eq!(Sensor::percent_to_measured_value(55.3), 5530);
        // Humidity can't be negative; clamp defensively.
        assert_eq!(Sensor::percent_to_measured_value(-1.0), 0);
    }
}
