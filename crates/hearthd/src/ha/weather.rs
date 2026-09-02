//! Translation from Home Assistant weather entities to the Matter data model.
//!
//! The Python shim reports a weather entity as a flat attribute bag (see
//! `homeassistant/components/weather/__init__.py`'s `async_send_state_to_rust`).
//! hearthd speaks Matter internally, so the bag is mapped here, at the
//! integration boundary, onto the same weather clusters the met.no integration
//! publishes.
//!
//! # Units
//!
//! Home Assistant reports each value in the *native* unit of whichever
//! integration produced it, and those differ: `met` reports wind in km/h while
//! others use m/s, and the same entity may report temperature in Fahrenheit.
//! The shim therefore sends the declared unit alongside each value and the
//! conversion happens here.
//!
//! A value whose unit is not recognised is dropped rather than guessed: a null
//! attribute is recoverable, a plausible-looking number that is wrong by a
//! factor of 3.6 is not. A value whose unit is absent falls back to Home
//! Assistant's metric default, which is what an integration that declares
//! nothing is reporting in.

use std::collections::HashMap;

use serde_json::Value;
use tracing::warn;

use crate::matter::CloudCoverCluster;
use crate::matter::Cluster;
use crate::matter::DewPointCluster;
use crate::matter::PrecipitationCluster;
use crate::matter::PressureMeasurementCluster;
use crate::matter::RelativeHumidityMeasurementCluster;
use crate::matter::TemperatureMeasurementCluster;
use crate::matter::UvIndexCluster;
use crate::matter::WeatherCondition;
use crate::matter::WeatherConditionCluster;
use crate::matter::WindMeasurementCluster;

/// The unit keys the shim sends, matching the `_attr_native_*_unit` names on a
/// Home Assistant weather entity.
const UNIT_TEMPERATURE: &str = "temperature";
const UNIT_PRESSURE: &str = "pressure";
const UNIT_WIND_SPEED: &str = "wind_speed";

/// Build the full set of weather clusters from a shim `state_update`.
///
/// Every cluster is always returned, even when its source attribute is absent
/// (its attributes are then null), so that a node's shape does not depend on
/// what a given Home Assistant integration happens to report.
///
/// `state` is Home Assistant's entity state, which for a weather entity is the
/// condition string; it is preferred over the `condition` attribute because an
/// integration whose condition getter raised still reports `"unknown"` there.
///
/// `units` maps an attribute name to the native unit it was reported in, as
/// declared by the entity.
pub fn clusters_from_state(
    state: &str,
    attributes: &Value,
    units: &HashMap<String, String>,
) -> Vec<Cluster> {
    let read = |key: &str| attributes.get(key).and_then(Value::as_f64);
    let unit = |key: &str| units.get(key).map(String::as_str);

    let temperature_unit = unit(UNIT_TEMPERATURE);
    let wind_speed_unit = unit(UNIT_WIND_SPEED);

    vec![
        Cluster::TemperatureMeasurement(TemperatureMeasurementCluster {
            measured_value: read("temperature")
                .and_then(|v| to_celsius("temperature", v, temperature_unit))
                .map(celsius_to_centi_i16),
        }),
        Cluster::PressureMeasurement(PressureMeasurementCluster {
            measured_value: read("pressure")
                .and_then(|v| to_hpa("pressure", v, unit(UNIT_PRESSURE)))
                .map(hpa_to_i16),
        }),
        // Relative humidity is a percentage in every Home Assistant
        // integration; there is no `_attr_native_humidity_unit` to declare.
        Cluster::RelativeHumidityMeasurement(RelativeHumidityMeasurementCluster {
            measured_value: read("humidity").map(percent_to_centi_u16),
        }),
        Cluster::WindMeasurement(WindMeasurementCluster {
            speed: read("wind_speed")
                .and_then(|v| to_mps("wind_speed", v, wind_speed_unit))
                .map(mps_to_centi_u16),
            gust: read("wind_gust")
                .and_then(|v| to_mps("wind_gust", v, wind_speed_unit))
                .map(mps_to_centi_u16),
            // Home Assistant allows a compass point ("NNW") here as well as a
            // bearing in degrees. Only the numeric form is modelled.
            bearing: read("wind_bearing").map(degrees_to_deci_u16),
        }),
        Cluster::CloudCover(CloudCoverCluster {
            cloud_area_fraction: read("cloud_coverage").map(percent_to_centi_u16),
        }),
        Cluster::DewPoint(DewPointCluster {
            measured_value: read("dew_point")
                .and_then(|v| to_celsius("dew_point", v, temperature_unit))
                .map(celsius_to_centi_i16),
        }),
        // The UV index is dimensionless.
        Cluster::UvIndex(UvIndexCluster {
            uv_index: read("uv_index").map(uv_to_deci_u16),
        }),
        // The shim carries precipitation only inside the forecast lists, never
        // as a current-conditions attribute, so this cluster is always null.
        // It is published anyway to keep the node's shape stable.
        Cluster::Precipitation(PrecipitationCluster::default()),
        Cluster::WeatherCondition(WeatherConditionCluster {
            condition: condition_from_ha(state),
        }),
    ]
}

/// The full set of weather clusters with every attribute null.
pub fn null_clusters() -> Vec<Cluster> {
    clusters_from_state("unknown", &Value::Null, &HashMap::new())
}

/// Normalise a temperature to degrees Celsius.
fn to_celsius(attribute: &str, value: f64, unit: Option<&str>) -> Option<f64> {
    match unit {
        None | Some("°C") => Some(value),
        Some("°F") => Some((value - 32.0) * 5.0 / 9.0),
        Some("K") => Some(value - 273.15),
        Some(other) => {
            unknown_unit(attribute, other);
            None
        }
    }
}

/// Normalise a pressure to hectopascals.
fn to_hpa(attribute: &str, value: f64, unit: Option<&str>) -> Option<f64> {
    match unit {
        None | Some("hPa") | Some("mbar") => Some(value),
        Some("kPa") => Some(value * 10.0),
        Some("inHg") => Some(value * 33.863_886_67),
        Some("mmHg") => Some(value * 1.333_223_684),
        Some("psi") => Some(value * 68.947_572_93),
        Some(other) => {
            unknown_unit(attribute, other);
            None
        }
    }
}

/// Normalise a speed to metres per second.
fn to_mps(attribute: &str, value: f64, unit: Option<&str>) -> Option<f64> {
    match unit {
        None | Some("m/s") => Some(value),
        Some("km/h") => Some(value / 3.6),
        Some("mph") => Some(value * 0.447_04),
        Some("kn") => Some(value * 0.514_444_444),
        Some("ft/s") => Some(value * 0.3048),
        Some(other) => {
            unknown_unit(attribute, other);
            None
        }
    }
}

/// Report a unit we cannot convert. The attribute is dropped, so the cluster
/// reads null rather than carrying a number in the wrong scale.
fn unknown_unit(attribute: &str, unit: &str) {
    warn!("dropping weather attribute '{attribute}': unrecognised unit '{unit}'");
}

/// Map a Home Assistant condition string to a normalised
/// [`WeatherCondition`].
///
/// Home Assistant's vocabulary is wider than hearthd's: `hail`, `exceptional`,
/// `windy` and `windy-variant` have no counterpart and become null rather than
/// being forced onto a neighbouring condition.
fn condition_from_ha(condition: &str) -> Option<WeatherCondition> {
    Some(match condition {
        "sunny" => WeatherCondition::ClearSky,
        "clear-night" => WeatherCondition::ClearNight,
        "partlycloudy" => WeatherCondition::PartlyCloudy,
        "cloudy" => WeatherCondition::Cloudy,
        "fog" => WeatherCondition::Fog,
        "rainy" => WeatherCondition::Rainy,
        "pouring" => WeatherCondition::Pouring,
        "lightning" | "lightning-rainy" => WeatherCondition::LightningRainy,
        "snowy" => WeatherCondition::Snowy,
        "snowy-rainy" => WeatherCondition::SnowyRainy,
        _ => return None,
    })
}

fn scale_clamp_i16(value: f64, scale: f64) -> i16 {
    (value * scale)
        .round()
        .clamp(i16::MIN as f64, i16::MAX as f64) as i16
}

fn scale_clamp_u16(value: f64, scale: f64) -> u16 {
    (value * scale).round().clamp(0.0, u16::MAX as f64) as u16
}

fn celsius_to_centi_i16(c: f64) -> i16 {
    scale_clamp_i16(c, 100.0)
}

/// Matter's pressure `MeasuredValue` is tenths of a kPa, which is numerically
/// hPa, so the value passes through unscaled.
fn hpa_to_i16(hpa: f64) -> i16 {
    scale_clamp_i16(hpa, 1.0)
}

fn percent_to_centi_u16(p: f64) -> u16 {
    scale_clamp_u16(p, 100.0)
}

fn mps_to_centi_u16(v: f64) -> u16 {
    scale_clamp_u16(v, 100.0)
}

fn degrees_to_deci_u16(d: f64) -> u16 {
    scale_clamp_u16(d, 10.0)
}

fn uv_to_deci_u16(u: f64) -> u16 {
    scale_clamp_u16(u, 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn find<'a>(clusters: &'a [Cluster], name: &str) -> &'a Cluster {
        clusters.iter().find(|c| c.name() == name).unwrap()
    }

    /// The units Home Assistant's own `met` integration declares, which is the
    /// integration this shim was built against.
    fn met_units() -> HashMap<String, String> {
        units(&[
            ("temperature", "°C"),
            ("pressure", "hPa"),
            ("wind_speed", "km/h"),
        ])
    }

    #[test]
    fn maps_a_full_attribute_bag() {
        let attributes = serde_json::json!({
            "condition": "partlycloudy",
            "temperature": 18.5,
            "humidity": 65.0,
            "pressure": 1013.2,
            "wind_speed": 11.52,
            "wind_bearing": 180.0,
            "wind_gust": 19.44,
            "cloud_coverage": 40.0,
            "dew_point": 12.1,
            "uv_index": 3.0,
        });

        let clusters = clusters_from_state("partlycloudy", &attributes, &met_units());

        assert!(matches!(
            find(&clusters, "TemperatureMeasurement"),
            Cluster::TemperatureMeasurement(c) if c.measured_value == Some(1850)
        ));
        assert!(matches!(
            find(&clusters, "PressureMeasurement"),
            Cluster::PressureMeasurement(c) if c.measured_value == Some(1013)
        ));
        assert!(matches!(
            find(&clusters, "RelativeHumidityMeasurement"),
            Cluster::RelativeHumidityMeasurement(c) if c.measured_value == Some(6500)
        ));
        // 11.52 km/h is 3.2 m/s; 19.44 km/h is 5.4 m/s.
        assert!(matches!(
            find(&clusters, "WindMeasurement"),
            Cluster::WindMeasurement(c)
                if c.speed == Some(320) && c.gust == Some(540) && c.bearing == Some(1800)
        ));
        assert!(matches!(
            find(&clusters, "CloudCover"),
            Cluster::CloudCover(c) if c.cloud_area_fraction == Some(4000)
        ));
        assert!(matches!(
            find(&clusters, "DewPoint"),
            Cluster::DewPoint(c) if c.measured_value == Some(1210)
        ));
        assert!(matches!(
            find(&clusters, "UvIndex"),
            Cluster::UvIndex(c) if c.uv_index == Some(30)
        ));
        assert!(matches!(
            find(&clusters, "WeatherCondition"),
            Cluster::WeatherCondition(c) if c.condition == Some(WeatherCondition::PartlyCloudy)
        ));
    }

    /// The bug this unit plumbing exists to prevent: `met` reports wind in
    /// km/h, and reading that as m/s turns a breeze into a hurricane.
    #[test]
    fn wind_speed_is_converted_from_the_declared_unit() {
        let attributes = serde_json::json!({ "wind_speed": 19.1, "wind_gust": 38.9 });

        let kmh = clusters_from_state("cloudy", &attributes, &met_units());
        assert!(matches!(
            find(&kmh, "WindMeasurement"),
            // 19.1 km/h = 5.31 m/s, 38.9 km/h = 10.81 m/s.
            Cluster::WindMeasurement(c) if c.speed == Some(531) && c.gust == Some(1081)
        ));

        let mps = clusters_from_state("cloudy", &attributes, &units(&[("wind_speed", "m/s")]));
        assert!(matches!(
            find(&mps, "WindMeasurement"),
            Cluster::WindMeasurement(c) if c.speed == Some(1910) && c.gust == Some(3890)
        ));
    }

    #[test]
    fn temperature_is_converted_from_the_declared_unit() {
        let attributes = serde_json::json!({ "temperature": 68.0, "dew_point": 50.0 });
        let clusters = clusters_from_state("sunny", &attributes, &units(&[("temperature", "°F")]));

        // 68 °F is 20 °C; 50 °F is 10 °C.
        assert!(matches!(
            find(&clusters, "TemperatureMeasurement"),
            Cluster::TemperatureMeasurement(c) if c.measured_value == Some(2000)
        ));
        assert!(matches!(
            find(&clusters, "DewPoint"),
            Cluster::DewPoint(c) if c.measured_value == Some(1000)
        ));
    }

    #[test]
    fn pressure_is_converted_from_the_declared_unit() {
        let attributes = serde_json::json!({ "pressure": 29.92 });
        let clusters = clusters_from_state("sunny", &attributes, &units(&[("pressure", "inHg")]));

        // 29.92 inHg is standard atmosphere, 1013 hPa.
        assert!(matches!(
            find(&clusters, "PressureMeasurement"),
            Cluster::PressureMeasurement(c) if c.measured_value == Some(1013)
        ));
    }

    /// An unrecognised unit is dropped, not guessed. A null attribute is
    /// recoverable; a number silently in the wrong scale is not.
    #[test]
    fn an_unrecognised_unit_drops_the_value() {
        let clusters = clusters_from_state(
            "cloudy",
            &serde_json::json!({ "wind_speed": 10.0, "temperature": 20.0 }),
            &units(&[("wind_speed", "furlongs/fortnight"), ("temperature", "°R")]),
        );

        assert!(matches!(
            find(&clusters, "WindMeasurement"),
            Cluster::WindMeasurement(c) if c.speed.is_none()
        ));
        assert!(matches!(
            find(&clusters, "TemperatureMeasurement"),
            Cluster::TemperatureMeasurement(c) if c.measured_value.is_none()
        ));
    }

    /// An integration that declares no units is reporting in Home Assistant's
    /// metric defaults.
    #[test]
    fn absent_units_fall_back_to_metric_defaults() {
        let clusters = clusters_from_state(
            "cloudy",
            &serde_json::json!({ "wind_speed": 3.2, "temperature": 18.5, "pressure": 1013.0 }),
            &HashMap::new(),
        );

        assert!(matches!(
            find(&clusters, "WindMeasurement"),
            Cluster::WindMeasurement(c) if c.speed == Some(320)
        ));
        assert!(matches!(
            find(&clusters, "TemperatureMeasurement"),
            Cluster::TemperatureMeasurement(c) if c.measured_value == Some(1850)
        ));
        assert!(matches!(
            find(&clusters, "PressureMeasurement"),
            Cluster::PressureMeasurement(c) if c.measured_value == Some(1013)
        ));
    }

    /// An integration that reports nothing still produces a full node, so a
    /// consumer never has to distinguish "absent cluster" from "null value".
    #[test]
    fn missing_attributes_become_null_not_missing_clusters() {
        let sparse = clusters_from_state("unknown", &serde_json::json!({}), &HashMap::new());

        assert_eq!(sparse.len(), null_clusters().len());
        for cluster in &sparse {
            match cluster {
                Cluster::TemperatureMeasurement(c) => assert!(c.measured_value.is_none()),
                Cluster::WeatherCondition(c) => assert!(c.condition.is_none()),
                Cluster::WindMeasurement(c) => assert!(c.speed.is_none() && c.bearing.is_none()),
                _ => {}
            }
        }
    }

    /// A compass point is the other shape Home Assistant permits for
    /// `wind_bearing`; it must not be read as a bearing of zero.
    #[test]
    fn a_textual_wind_bearing_is_dropped() {
        let clusters = clusters_from_state(
            "cloudy",
            &serde_json::json!({ "wind_bearing": "NNW", "wind_speed": 3.0 }),
            &units(&[("wind_speed", "m/s")]),
        );

        assert!(matches!(
            find(&clusters, "WindMeasurement"),
            Cluster::WindMeasurement(c) if c.bearing.is_none() && c.speed == Some(300)
        ));
    }

    /// Conditions hearthd does not model are null rather than approximated.
    #[test]
    fn unmodelled_conditions_are_null() {
        for condition in ["hail", "exceptional", "windy", "windy-variant", "unknown"] {
            assert_eq!(condition_from_ha(condition), None, "{condition}");
        }
    }
}
