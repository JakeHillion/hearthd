//! Dyson device state parsed from MQTT JSON payloads.
//!
//! The TP07 (Pure Cool, device type 438) uses the v2 state format. Fields are
//! single strings or `["value", "old_value"]` pairs. We extract the current
//! value from either form.

use serde_json::Value;

/// Parsed device state for a TP07 Pure Cool.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PureCoolState {
    pub fan_power: Option<bool>,
    pub fan_speed: Option<u8>, // 1-10, or None when in auto mode
    pub auto_mode: Option<bool>,
    pub oscillation: Option<bool>,
    pub night_mode: Option<bool>,
    pub continuous_monitoring: Option<bool>,
    pub front_airflow: Option<bool>,
    pub sleep_timer: Option<u16>,

    // Environmental
    pub temperature_kelvin: Option<f64>,
    pub humidity_percent: Option<u8>,
    pub pm2_5: Option<u16>,
    pub pm10: Option<u16>,
    pub no2: Option<u16>,
    pub voc: Option<u16>,

    // Filter / diagnostics
    pub filter_life: Option<u8>,
}

impl PureCoolState {
    /// Update state from a `CURRENT-STATE` or `STATE-CHANGE` payload.
    pub fn apply_state_payload(&mut self, payload: &Value) {
        let Some(product_state) = payload.get("product-state").and_then(|v| v.as_object()) else {
            return;
        };

        for (key, value) in product_state {
            match key.as_str() {
                "fpwr" => self.fan_power = parse_on_off(value),
                "fnsp" => self.fan_speed = parse_speed(value),
                "auto" => self.auto_mode = parse_on_off(value),
                "oson" => self.oscillation = parse_on_off(value),
                "nmod" => self.night_mode = parse_on_off(value),
                "rhtm" => self.continuous_monitoring = parse_on_off(value),
                "fdir" => self.front_airflow = parse_on_off(value),
                "sltm" => self.sleep_timer = parse_timer(value),
                "filf" => self.filter_life = parse_filter_life(value),
                _ => {}
            }
        }
    }

    /// Update environmental readings from a `ENVIRONMENTAL-CURRENT-SENSOR-DATA` payload.
    pub fn apply_environmental_payload(&mut self, payload: &Value) {
        let Some(data) = payload.get("data").and_then(|v| v.as_object()) else {
            return;
        };

        for (key, value) in data {
            match key.as_str() {
                "tact" => self.temperature_kelvin = parse_temperature(value),
                "hact" => self.humidity_percent = parse_humidity(value),
                "pm25" => self.pm2_5 = parse_air_quality(value),
                "pm10" => self.pm10 = parse_air_quality(value),
                "noxl" => self.no2 = parse_air_quality(value),
                "va10" => self.voc = parse_air_quality(value),
                _ => {}
            }
        }
    }
}

fn current_value(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => Some(s.as_str()),
        Value::Array(arr) if arr.len() == 2 => arr[0].as_str(),
        _ => None,
    }
}

fn parse_on_off(value: &Value) -> Option<bool> {
    match current_value(value)? {
        "ON" | "FAN" | "HSTR" | "HEAT" => Some(true),
        "OFF" | "OIOF" => Some(false),
        _ => None,
    }
}

fn parse_speed(value: &Value) -> Option<u8> {
    let s = current_value(value)?;
    if s.eq_ignore_ascii_case("AUTO") {
        return None;
    }
    s.parse::<u8>().ok()
}

fn parse_timer(value: &Value) -> Option<u16> {
    let s = current_value(value)?;
    if s.eq_ignore_ascii_case("OFF") {
        return Some(0);
    }
    s.parse::<u16>().ok()
}

fn parse_filter_life(value: &Value) -> Option<u8> {
    current_value(value)?.parse::<u8>().ok()
}

fn parse_temperature(value: &Value) -> Option<f64> {
    let s = current_value(value)?;
    if s.eq_ignore_ascii_case("OFF") || s.eq_ignore_ascii_case("INIT") {
        return None;
    }
    s.parse::<f64>().ok().map(|v| v / 10.0)
}

fn parse_humidity(value: &Value) -> Option<u8> {
    let s = current_value(value)?;
    if s.eq_ignore_ascii_case("OFF") || s.eq_ignore_ascii_case("INIT") {
        return None;
    }
    s.parse::<u8>().ok()
}

fn parse_air_quality(value: &Value) -> Option<u16> {
    let s = current_value(value)?;
    if s.eq_ignore_ascii_case("OFF")
        || s.eq_ignore_ascii_case("INIT")
        || s.eq_ignore_ascii_case("FAIL")
    {
        return None;
    }
    s.parse::<u16>().ok()
}
