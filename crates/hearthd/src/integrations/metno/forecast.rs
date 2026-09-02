//! met.no `locationforecast/2.0/complete` response types and their translation
//! into the Matter-shaped clusters hearthd speaks internally.
//!
//! Only the current conditions (the nearest timeseries entry) are modelled;
//! forecasts are out of scope for now. All translation from met.no's native
//! floating-point SI units into Matter's scaled integers happens here, at the
//! integration boundary.

use serde::Deserialize;

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

/// Deserialized subset of the met.no locationforecast response.
#[derive(Debug, Clone, Deserialize)]
pub struct ForecastResponse {
    pub properties: Properties,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Properties {
    #[serde(default)]
    pub timeseries: Vec<TimeSeriesEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimeSeriesEntry {
    pub data: TimeSeriesData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimeSeriesData {
    #[serde(default)]
    pub instant: Instant,
    /// Forecast summary/details for the hour following this entry. Absent on
    /// entries near the end of the series.
    #[serde(default)]
    pub next_1_hours: Option<NextHours>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Instant {
    #[serde(default)]
    pub details: InstantDetails,
}

/// Instantaneous measurements. All fields are optional: met.no omits
/// parameters it has no value for.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InstantDetails {
    pub air_temperature: Option<f64>,
    pub air_pressure_at_sea_level: Option<f64>,
    pub relative_humidity: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_speed_of_gust: Option<f64>,
    pub wind_from_direction: Option<f64>,
    pub cloud_area_fraction: Option<f64>,
    pub dew_point_temperature: Option<f64>,
    pub ultraviolet_index_clear_sky: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NextHours {
    #[serde(default)]
    pub summary: Summary,
    #[serde(default)]
    pub details: NextHoursDetails,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Summary {
    /// met.no weather symbol, e.g. `partlycloudy_day`.
    pub symbol_code: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NextHoursDetails {
    pub precipitation_amount: Option<f64>,
    pub probability_of_precipitation: Option<f64>,
}

impl ForecastResponse {
    /// Build the full set of current-condition clusters from the nearest
    /// timeseries entry.
    ///
    /// Every weather cluster is always returned, even when its source values
    /// are absent (its attributes are then null), so that a node's shape does
    /// not depend on what met.no happened to report.
    pub fn current_clusters(&self) -> Vec<Cluster> {
        let entry = self.properties.timeseries.first();
        let details = entry
            .map(|e| e.data.instant.details.clone())
            .unwrap_or_default();
        let next = entry.and_then(|e| e.data.next_1_hours.as_ref());

        vec![
            Cluster::TemperatureMeasurement(TemperatureMeasurementCluster {
                measured_value: details.air_temperature.map(celsius_to_centi_i16),
            }),
            Cluster::PressureMeasurement(PressureMeasurementCluster {
                measured_value: details.air_pressure_at_sea_level.map(hpa_to_i16),
            }),
            Cluster::RelativeHumidityMeasurement(RelativeHumidityMeasurementCluster {
                measured_value: details.relative_humidity.map(percent_to_centi_u16),
            }),
            Cluster::WindMeasurement(WindMeasurementCluster {
                speed: details.wind_speed.map(mps_to_centi_u16),
                gust: details.wind_speed_of_gust.map(mps_to_centi_u16),
                bearing: details.wind_from_direction.map(degrees_to_deci_u16),
            }),
            Cluster::CloudCover(CloudCoverCluster {
                cloud_area_fraction: details.cloud_area_fraction.map(percent_to_centi_u16),
            }),
            Cluster::DewPoint(DewPointCluster {
                measured_value: details.dew_point_temperature.map(celsius_to_centi_i16),
            }),
            Cluster::UvIndex(UvIndexCluster {
                uv_index: details.ultraviolet_index_clear_sky.map(uv_to_deci_u16),
            }),
            Cluster::Precipitation(PrecipitationCluster {
                amount: next
                    .and_then(|n| n.details.precipitation_amount)
                    .map(mm_to_deci_u16),
                probability: next
                    .and_then(|n| n.details.probability_of_precipitation)
                    .map(percent_to_centi_u16),
            }),
            Cluster::WeatherCondition(WeatherConditionCluster {
                condition: next
                    .and_then(|n| n.summary.symbol_code.as_deref())
                    .and_then(symbol_code_to_condition),
            }),
        ]
    }
}

/// The full set of weather clusters with every attribute null.
pub fn null_clusters() -> Vec<Cluster> {
    ForecastResponse {
        properties: Properties {
            timeseries: Vec::new(),
        },
    }
    .current_clusters()
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

fn mm_to_deci_u16(mm: f64) -> u16 {
    scale_clamp_u16(mm, 10.0)
}

/// Map a met.no weather symbol code (e.g. `heavyrainshowersandthunder_day`)
/// to a normalised [`WeatherCondition`].
///
/// Codes carry an optional `_day` / `_night` / `_polartwilight` variant; only
/// `clearsky` distinguishes day from night. Any code containing `andthunder`
/// maps to lightning regardless of intensity, mirroring Home Assistant.
fn symbol_code_to_condition(code: &str) -> Option<WeatherCondition> {
    let (base, night) = match code.rsplit_once('_') {
        Some((b, "night")) => (b, true),
        Some((b, "day")) | Some((b, "polartwilight")) => (b, false),
        _ => (code, false),
    };

    if base.contains("andthunder") {
        return Some(WeatherCondition::LightningRainy);
    }

    Some(match base {
        "clearsky" => {
            if night {
                WeatherCondition::ClearNight
            } else {
                WeatherCondition::ClearSky
            }
        }
        "fair" | "partlycloudy" => WeatherCondition::PartlyCloudy,
        "cloudy" => WeatherCondition::Cloudy,
        "fog" => WeatherCondition::Fog,
        "lightrain" | "lightrainshowers" | "rain" | "rainshowers" => WeatherCondition::Rainy,
        "heavyrain" | "heavyrainshowers" => WeatherCondition::Pouring,
        "lightsnow" | "lightsnowshowers" | "snow" | "snowshowers" | "heavysnow"
        | "heavysnowshowers" => WeatherCondition::Snowy,
        "lightsleet" | "lightsleetshowers" | "sleet" | "sleetshowers" | "heavysleet"
        | "heavysleetshowers" => WeatherCondition::SnowyRainy,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed but representative `complete` response.
    const SAMPLE: &str = r#"{
      "properties": {
        "timeseries": [
          {
            "time": "2026-08-14T14:00:00Z",
            "data": {
              "instant": {
                "details": {
                  "air_temperature": 18.5,
                  "air_pressure_at_sea_level": 1013.2,
                  "relative_humidity": 65.0,
                  "wind_speed": 3.2,
                  "wind_speed_of_gust": 5.4,
                  "wind_from_direction": 180.0,
                  "cloud_area_fraction": 40.0,
                  "dew_point_temperature": 12.1,
                  "ultraviolet_index_clear_sky": 3.0
                }
              },
              "next_1_hours": {
                "summary": { "symbol_code": "partlycloudy_day" },
                "details": {
                  "precipitation_amount": 0.4,
                  "probability_of_precipitation": 25.0
                }
              }
            }
          }
        ]
      }
    }"#;

    fn parse(s: &str) -> ForecastResponse {
        serde_json::from_str(s).unwrap()
    }

    fn find<'a>(clusters: &'a [Cluster], name: &str) -> &'a Cluster {
        clusters.iter().find(|c| c.name() == name).unwrap()
    }

    #[test]
    fn maps_all_current_conditions_with_scaling() {
        let clusters = parse(SAMPLE).current_clusters();

        assert_eq!(clusters.len(), 9);

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
            find(&clusters, "Precipitation"),
            Cluster::Precipitation(c) if c.amount == Some(4) && c.probability == Some(2500)
        ));
        assert!(matches!(
            find(&clusters, "WeatherCondition"),
            Cluster::WeatherCondition(c) if c.condition == Some(WeatherCondition::PartlyCloudy)
        ));
    }

    #[test]
    fn empty_timeseries_yields_all_null_clusters() {
        let clusters = parse(r#"{"properties":{"timeseries":[]}}"#).current_clusters();
        assert_eq!(clusters.len(), 9);
        for cluster in clusters {
            match cluster {
                Cluster::TemperatureMeasurement(c) => assert!(c.measured_value.is_none()),
                Cluster::WeatherCondition(c) => assert!(c.condition.is_none()),
                Cluster::WindMeasurement(c) => assert!(c.speed.is_none() && c.bearing.is_none()),
                _ => {}
            }
        }
    }

    #[test]
    fn missing_next_hours_leaves_forecast_clusters_null() {
        let json = r#"{
          "properties": { "timeseries": [ { "time": "t", "data": {
            "instant": { "details": { "air_temperature": 1.0 } }
          } } ] }
        }"#;
        let clusters = parse(json).current_clusters();
        assert!(matches!(
            find(&clusters, "Precipitation"),
            Cluster::Precipitation(c) if c.amount.is_none() && c.probability.is_none()
        ));
        assert!(matches!(
            find(&clusters, "WeatherCondition"),
            Cluster::WeatherCondition(c) if c.condition.is_none()
        ));
    }

    #[test]
    fn negative_temperature_scales_and_clamps() {
        assert_eq!(celsius_to_centi_i16(-5.0), -500);
        assert_eq!(celsius_to_centi_i16(21.345), 2135);
        assert_eq!(celsius_to_centi_i16(100_000.0), i16::MAX);
    }

    #[test]
    fn condition_mapping_handles_day_night_thunder_and_unknown() {
        assert_eq!(
            symbol_code_to_condition("clearsky_day"),
            Some(WeatherCondition::ClearSky)
        );
        assert_eq!(
            symbol_code_to_condition("clearsky_night"),
            Some(WeatherCondition::ClearNight)
        );
        assert_eq!(
            symbol_code_to_condition("clearsky_polartwilight"),
            Some(WeatherCondition::ClearSky)
        );
        assert_eq!(
            symbol_code_to_condition("fair_night"),
            Some(WeatherCondition::PartlyCloudy)
        );
        assert_eq!(
            symbol_code_to_condition("heavyrain"),
            Some(WeatherCondition::Pouring)
        );
        assert_eq!(
            symbol_code_to_condition("lightrainshowersandthunder_day"),
            Some(WeatherCondition::LightningRainy)
        );
        assert_eq!(
            symbol_code_to_condition("sleet"),
            Some(WeatherCondition::SnowyRainy)
        );
        assert_eq!(symbol_code_to_condition("nonsense"), None);
    }
}
