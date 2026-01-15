//! Weather entity for hearthd
//!
//! Stores weather state received from HA weather integrations and serializes
//! it to JSON for the Engine.

use serde::Serialize;

use super::entity::Entity;

/// Current weather state.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WeatherState {
    pub condition: Option<String>,
    pub temperature: Option<f64>,
    pub humidity: Option<f64>,
    pub pressure: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_bearing: Option<f64>,
    pub wind_gust: Option<f64>,
    pub cloud_coverage: Option<f64>,
    pub dew_point: Option<f64>,
    pub uv_index: Option<f64>,
}

/// A single forecast entry (daily or hourly).
#[derive(Debug, Clone, Serialize)]
pub struct Forecast {
    pub datetime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub templow: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub humidity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precipitation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precipitation_probability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_bearing: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_gust_speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_coverage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uv_index: Option<f64>,
}

/// Weather entity that stores weather data from HA integrations.
pub struct Weather {
    pub entity_id: String,
    pub name: String,
    pub state: WeatherState,
    pub forecast_daily: Vec<Forecast>,
    pub forecast_hourly: Vec<Forecast>,
}

impl Weather {
    pub fn new(entity_id: String, name: String) -> Self {
        Self {
            entity_id,
            name,
            state: WeatherState::default(),
            forecast_daily: Vec::new(),
            forecast_hourly: Vec::new(),
        }
    }

    /// Update weather state from a JSON attributes object sent by Python.
    pub fn update_from_attributes(&mut self, attrs: &serde_json::Value) {
        if let Some(v) = attrs.get("condition").and_then(|v| v.as_str()) {
            self.state.condition = Some(v.to_string());
        }
        if let Some(v) = attrs.get("temperature").and_then(|v| v.as_f64()) {
            self.state.temperature = Some(v);
        }
        if let Some(v) = attrs.get("humidity").and_then(|v| v.as_f64()) {
            self.state.humidity = Some(v);
        }
        if let Some(v) = attrs.get("pressure").and_then(|v| v.as_f64()) {
            self.state.pressure = Some(v);
        }
        if let Some(v) = attrs.get("wind_speed").and_then(|v| v.as_f64()) {
            self.state.wind_speed = Some(v);
        }
        if let Some(v) = attrs.get("wind_bearing").and_then(|v| v.as_f64()) {
            self.state.wind_bearing = Some(v);
        }
        if let Some(v) = attrs.get("wind_gust").and_then(|v| v.as_f64()) {
            self.state.wind_gust = Some(v);
        }
        if let Some(v) = attrs.get("cloud_coverage").and_then(|v| v.as_f64()) {
            self.state.cloud_coverage = Some(v);
        }
        if let Some(v) = attrs.get("dew_point").and_then(|v| v.as_f64()) {
            self.state.dew_point = Some(v);
        }
        if let Some(v) = attrs.get("uv_index").and_then(|v| v.as_f64()) {
            self.state.uv_index = Some(v);
        }

        // Parse daily forecasts
        if let Some(daily) = attrs.get("forecast_daily").and_then(|v| v.as_array()) {
            self.forecast_daily = daily.iter().filter_map(parse_forecast).collect();
        }

        // Parse hourly forecasts
        if let Some(hourly) = attrs.get("forecast_hourly").and_then(|v| v.as_array()) {
            self.forecast_hourly = hourly.iter().filter_map(parse_forecast).collect();
        }
    }
}

fn parse_forecast(v: &serde_json::Value) -> Option<Forecast> {
    let datetime = v.get("datetime")?.as_str()?.to_string();
    Some(Forecast {
        datetime,
        condition: v.get("condition").and_then(|v| v.as_str()).map(String::from),
        temperature: v
            .get("native_temperature")
            .or_else(|| v.get("temperature"))
            .and_then(|v| v.as_f64()),
        templow: v
            .get("native_templow")
            .or_else(|| v.get("templow"))
            .and_then(|v| v.as_f64()),
        humidity: v.get("humidity").and_then(|v| v.as_f64()),
        precipitation: v
            .get("native_precipitation")
            .or_else(|| v.get("precipitation"))
            .and_then(|v| v.as_f64()),
        precipitation_probability: v
            .get("precipitation_probability")
            .and_then(|v| v.as_f64()),
        pressure: v
            .get("native_pressure")
            .or_else(|| v.get("pressure"))
            .and_then(|v| v.as_f64()),
        wind_speed: v
            .get("native_wind_speed")
            .or_else(|| v.get("wind_speed"))
            .and_then(|v| v.as_f64()),
        wind_bearing: v.get("wind_bearing").and_then(|v| v.as_f64()),
        wind_gust_speed: v
            .get("native_wind_gust_speed")
            .or_else(|| v.get("wind_gust_speed"))
            .and_then(|v| v.as_f64()),
        cloud_coverage: v.get("cloud_coverage").and_then(|v| v.as_f64()),
        uv_index: v.get("uv_index").and_then(|v| v.as_f64()),
    })
}

impl Entity for Weather {
    fn state_json(&self) -> serde_json::Value {
        serde_json::json!({
            "entity_id": self.entity_id,
            "name": self.name,
            "platform": "weather",
            "state": self.state,
            "forecast_daily": self.forecast_daily,
            "forecast_hourly": self.forecast_hourly,
        })
    }

    fn platform(&self) -> &'static str {
        "weather"
    }

    fn update_from_ha_state(&mut self, _state: &str, attributes: &serde_json::Value) {
        self.update_from_attributes(attributes);
    }
}
