use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub locations: LocationsConfig,
    #[serde(default)]
    pub integrations: IntegrationsConfig,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Trace => LevelFilter::TRACE,
            LogLevel::Debug => LevelFilter::DEBUG,
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Warn => LevelFilter::WARN,
            LogLevel::Error => LevelFilter::ERROR,
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct LoggingConfig {
    /// Log level: trace, debug, info, warn, error
    #[serde(default)]
    pub level: LogLevel,

    #[serde(default)]
    pub overrides: HashMap<String, LogLevel>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct LocationsConfig {
    /// The default location to use
    pub default: Option<String>,

    /// Named locations with their configurations
    #[serde(flatten)]
    pub locations: HashMap<String, Location>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Location {
    /// Latitude in decimal degrees
    pub latitude: f64,

    /// Longitude in decimal degrees
    pub longitude: f64,

    /// Elevation in meters (optional, can be inferred)
    pub elevation_m: Option<f64>,

    /// Timezone identifier (optional, can be inferred from lat/long)
    pub timezone: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct IntegrationsConfig {
    // Empty for now - integrations will be added as static fields later
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_str(&contents)
    }

    /// Parse configuration from a TOML string
    pub fn from_str(contents: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config: Config = toml::from_str(contents)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate that default location exists if specified
        if let Some(ref default) = self.locations.default {
            if !self.locations.locations.contains_key(default) {
                return Err(format!(
                    "default location '{}' not found in locations",
                    default
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_config() {
        let toml = r#"
            [logging]
            level = "debug"

            [locations]

            [integrations]
        "#;

        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.logging.level, LogLevel::Debug);
    }

    #[test]
    fn test_config_with_locations() {
        let toml = r#"
            [logging]
            level = "info"

            [locations]
            default = "home"

            [locations.home]
            latitude = 59.9139
            longitude = 10.7522
            elevation_m = 10
            timezone = "Europe/Oslo"

            [locations.work]
            latitude = 59.9110
            longitude = 10.7579

            [integrations]
        "#;

        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.logging.level, LogLevel::Info);
        assert_eq!(config.locations.default, Some("home".to_string()));
        assert!(config.locations.locations.contains_key("home"));
        assert!(config.locations.locations.contains_key("work"));

        // Verify home location details
        let home = &config.locations.locations["home"];
        assert_eq!(home.latitude, 59.9139);
        assert_eq!(home.longitude, 10.7522);
        assert_eq!(home.elevation_m, Some(10.0));
        assert_eq!(home.timezone, Some("Europe/Oslo".to_string()));

        // Verify work location has optional fields as None
        let work = &config.locations.locations["work"];
        assert_eq!(work.latitude, 59.9110);
        assert_eq!(work.longitude, 10.7579);
        assert_eq!(work.elevation_m, None);
        assert_eq!(work.timezone, None);
    }

    #[test]
    fn test_invalid_default_location() {
        let toml = r#"
            [logging]
            level = "info"

            [locations]
            default = "nonexistent"

            [locations.home]
            latitude = 59.9139
            longitude = 10.7522

            [integrations]
        "#;

        let result = Config::from_str(toml);
        assert!(result.is_err());
    }
}
