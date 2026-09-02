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
//! The shim forwards each value in the entity's *native* unit but does not
//! forward the unit itself, so there is nothing to convert from. The
//! conversions below therefore assume Home Assistant's metric defaults —
//! degrees Celsius, hectopascals, metres per second, percent and degrees —
//! which is what the met.no integration this shim was built against reports.
//! An integration with imperial native units would be misread; forwarding
//! `native_*_unit` over the protocol is the fix, and is not implemented.

use serde_json::Value;

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

/// Build the full set of weather clusters from a shim `state_update`.
///
/// Every cluster is always returned, even when its source attribute is absent
/// (its attributes are then null), so that a node's shape does not depend on
/// what a given Home Assistant integration happens to report.
///
/// `state` is Home Assistant's entity state, which for a weather entity is the
/// condition string; it is preferred over the `condition` attribute because an
/// integration whose condition getter raised still reports `"unknown"` there.
pub fn clusters_from_state(state: &str, attributes: &Value) -> Vec<Cluster> {
    let f = |key: &str| attributes.get(key).and_then(Value::as_f64);

    vec![
        Cluster::TemperatureMeasurement(TemperatureMeasurementCluster {
            measured_value: f("temperature").map(celsius_to_centi_i16),
        }),
        Cluster::PressureMeasurement(PressureMeasurementCluster {
            measured_value: f("pressure").map(hpa_to_i16),
        }),
        Cluster::RelativeHumidityMeasurement(RelativeHumidityMeasurementCluster {
            measured_value: f("humidity").map(percent_to_centi_u16),
        }),
        Cluster::WindMeasurement(WindMeasurementCluster {
            speed: f("wind_speed").map(mps_to_centi_u16),
            gust: f("wind_gust").map(mps_to_centi_u16),
            // Home Assistant allows a compass point ("NNW") here as well as a
            // bearing in degrees. Only the numeric form is modelled.
            bearing: f("wind_bearing").map(degrees_to_deci_u16),
        }),
        Cluster::CloudCover(CloudCoverCluster {
            cloud_area_fraction: f("cloud_coverage").map(percent_to_centi_u16),
        }),
        Cluster::DewPoint(DewPointCluster {
            measured_value: f("dew_point").map(celsius_to_centi_i16),
        }),
        Cluster::UvIndex(UvIndexCluster {
            uv_index: f("uv_index").map(uv_to_deci_u16),
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
    clusters_from_state("unknown", &Value::Null)
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

    fn find<'a>(clusters: &'a [Cluster], name: &str) -> &'a Cluster {
        clusters.iter().find(|c| c.name() == name).unwrap()
    }

    #[test]
    fn maps_a_full_attribute_bag() {
        let attributes = serde_json::json!({
            "condition": "partlycloudy",
            "temperature": 18.5,
            "humidity": 65.0,
            "pressure": 1013.2,
            "wind_speed": 3.2,
            "wind_bearing": 180.0,
            "wind_gust": 5.4,
            "cloud_coverage": 40.0,
            "dew_point": 12.1,
            "uv_index": 3.0,
        });

        let clusters = clusters_from_state("partlycloudy", &attributes);

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

    /// An integration that reports nothing still produces a full node, so a
    /// consumer never has to distinguish "absent cluster" from "null value".
    #[test]
    fn missing_attributes_become_null_not_missing_clusters() {
        let sparse = clusters_from_state("unknown", &serde_json::json!({}));

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
