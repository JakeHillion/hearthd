//! Configuration file parsing and structures.
//!
//! hearthd uses TOML for declarative configuration with two types of integrations:
//! - Native integrations: Statically typed Rust structs (e.g., MQTT, HTTP API)
//! - Home Assistant integrations: Dynamically typed, validated by Python code

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Top-level configuration structure
#[derive(Debug, Deserialize)]
pub struct Config {
    pub system: SystemConfig,
    pub location: LocationConfig,
    pub integrations: IntegrationsConfig,
}

/// System-wide configuration
#[derive(Debug, Deserialize)]
pub struct SystemConfig {
    /// Path to Python interpreter
    pub python_path: PathBuf,

    /// Log level (trace, debug, info, warn, error)
    pub log_level: String,

    /// Path to Home Assistant core source checkout
    /// Used to import integrations from homeassistant.components
    /// Our shims take priority for core modules
    pub ha_source_path: PathBuf,
}

/// Global location configuration
///
/// Provides defaults for location-based integrations
#[derive(Debug, Deserialize)]
pub struct LocationConfig {
    /// Latitude in decimal degrees
    pub latitude: f64,

    /// Longitude in decimal degrees
    pub longitude: f64,

    /// Elevation in meters
    pub elevation: i32,

    /// IANA timezone identifier (e.g., "Europe/Oslo")
    pub timezone: String,
}

/// Integration configuration container
#[derive(Debug, Deserialize)]
pub struct IntegrationsConfig {
    /// Native MQTT integration (statically typed)
    #[serde(default)]
    pub mqtt: Option<MqttConfig>,

    /// Native HTTP API integration (statically typed)
    #[serde(default)]
    pub api: Option<ApiConfig>,

    /// Home Assistant integrations (dynamically typed)
    /// Key = entry_id, Value = integration config
    #[serde(default)]
    pub ha: HashMap<String, HaIntegrationConfig>,
}

/// Native MQTT integration configuration
#[derive(Debug, Deserialize)]
pub struct MqttConfig {
    pub enabled: bool,
    pub broker: String,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub password: Option<String>,
}

/// Native HTTP API integration configuration
#[derive(Debug, Deserialize)]
pub struct ApiConfig {
    pub enabled: bool,
    pub bind: String,

    #[serde(default)]
    pub auth_token: Option<String>,
}

/// Home Assistant integration configuration
///
/// The config field is intentionally opaque (toml::Value) and will be
/// validated by the Python integration code at runtime.
#[derive(Debug, Deserialize)]
pub struct HaIntegrationConfig {
    /// Home Assistant domain name (e.g., "met", "accuweather")
    pub domain: String,

    /// Whether this integration is enabled
    pub enabled: bool,

    /// Opaque configuration passed to Python integration
    /// This will be converted to serde_json::Value before sending to Python
    pub config: toml::Value,
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path.as_ref())
            .map_err(|e| ConfigError::Io(path.as_ref().to_path_buf(), e))?;

        toml::from_str(&contents).map_err(ConfigError::Parse)
    }
}

impl HaIntegrationConfig {
    /// Convert the opaque TOML config to JSON for transmission to Python
    pub fn config_to_json(&self) -> Result<serde_json::Value, ConfigError> {
        // Convert toml::Value -> serde_json::Value
        let json_str = serde_json::to_string(&self.config)
            .map_err(ConfigError::JsonConversion)?;

        serde_json::from_str(&json_str).map_err(ConfigError::JsonConversion)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),

    #[error("Failed to parse TOML: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("Failed to convert config to JSON: {0}")]
    JsonConversion(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let toml = r#"
            [system]
            python_path = "/usr/bin/python3"
            log_level = "info"

            [location]
            latitude = 59.9139
            longitude = 10.7522
            elevation = 10
            timezone = "Europe/Oslo"

            [integrations]
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.system.log_level, "info");
        assert_eq!(config.location.latitude, 59.9139);
        assert!(config.integrations.ha.is_empty());
    }

    #[test]
    fn test_parse_ha_integration() {
        let toml = r#"
            [system]
            python_path = "/usr/bin/python3"
            log_level = "info"

            [location]
            latitude = 59.9139
            longitude = 10.7522
            elevation = 10
            timezone = "Europe/Oslo"

            [integrations]

            [integrations.ha.met_oslo]
            domain = "met"
            enabled = true
            config.latitude = 59.9139
            config.longitude = 10.7522
            config.elevation = 10
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.integrations.ha.len(), 1);

        let met_config = config.integrations.ha.get("met_oslo").unwrap();
        assert_eq!(met_config.domain, "met");
        assert!(met_config.enabled);

        let json = met_config.config_to_json().unwrap();
        assert_eq!(json["latitude"], 59.9139);
    }

    #[test]
    fn test_parse_native_integration() {
        let toml = r#"
            [system]
            python_path = "/usr/bin/python3"
            log_level = "debug"

            [location]
            latitude = 59.9139
            longitude = 10.7522
            elevation = 10
            timezone = "Europe/Oslo"

            [integrations]

            [integrations.mqtt]
            enabled = true
            broker = "localhost:1883"
            username = "hearthd"
        "#;

        let config: Config = toml::from_str(toml).unwrap();

        let mqtt = config.integrations.mqtt.as_ref().unwrap();
        assert!(mqtt.enabled);
        assert_eq!(mqtt.broker, "localhost:1883");
        assert_eq!(mqtt.username.as_ref().unwrap(), "hearthd");
    }
}
