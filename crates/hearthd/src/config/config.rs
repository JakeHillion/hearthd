use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use tracing_subscriber::filter::LevelFilter;

use super::diagnostics::{format_diagnostics, Diagnostic, Error, SourceInfo, ValidationError};
use super::partial::{PartialConfig, PartialLocation};

#[derive(Debug, Default)]
pub struct Config {
    pub logging: LoggingConfig,
    pub locations: LocationsConfig,
    #[allow(dead_code)]
    pub integrations: IntegrationsConfig,
}

// LogLevel needs Deserialize because it's used in PartialLoggingConfig with toml::Spanned
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
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

#[derive(Debug, Default)]
pub struct LoggingConfig {
    /// Log level: trace, debug, info, warn, error
    pub level: LogLevel,

    pub overrides: HashMap<String, LogLevel>,
}

#[derive(Debug, Default)]
pub struct LocationsConfig {
    /// The default location to use
    pub default: Option<String>,

    /// Named locations with their configurations
    pub locations: HashMap<String, Location>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields are used by application code, not in tests
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

#[derive(Debug, Default)]
pub struct IntegrationsConfig {
    // Empty for now - integrations will be added as static fields later
}

impl Config {
    /// Load configuration from multiple TOML files with import resolution
    ///
    /// This is the new recommended way to load configs. It supports:
    /// - Multiple config files (e.g., base + secrets)
    /// - Import statements within config files
    /// - Conflict detection across all sources
    /// - Validation with all errors and warnings reported together
    ///
    /// Returns Ok((Config, diagnostics)) where diagnostics contains warnings and errors.
    /// Only returns Err if there are actual errors (not just warnings).
    pub fn from_files(
        paths: &[PathBuf],
    ) -> Result<(Self, Vec<Diagnostic>), Box<dyn std::error::Error>> {
        // Load all configs
        let configs = PartialConfig::load_with_imports(paths)?;

        // Merge with first-wins semantics, collecting diagnostics
        let (partial, diagnostics) = PartialConfig::merge(configs);

        // Convert to Config and validate, combining all diagnostics
        Self::from_partial(partial, diagnostics)
    }

    /// Convert a PartialConfig to a Config, validating all fields
    ///
    /// Takes diagnostics from the merge step and adds validation diagnostics.
    /// Returns Ok((Config, diagnostics)) if no errors, Err if there are errors.
    pub fn from_partial(
        partial: PartialConfig,
        mut diagnostics: Vec<Diagnostic>,
    ) -> Result<(Self, Vec<Diagnostic>), Box<dyn std::error::Error>> {

        // Convert logging config
        let logging = if let Some(partial_logging) = partial.logging {
            LoggingConfig {
                level: partial_logging
                    .level
                    .map(|s| *s.get_ref())
                    .unwrap_or_default(),
                overrides: partial_logging
                    .overrides
                    .map(|hm| hm.into_iter().map(|(k, v)| (k, *v.get_ref())).collect())
                    .unwrap_or_default(),
            }
        } else {
            LoggingConfig::default()
        };

        // Convert locations config
        let locations = if let Some(partial_locations) = partial.locations {
            // Validate and convert each location
            let mut locations_map = HashMap::new();
            for (key, partial_location) in partial_locations.locations {
                match Self::validate_location(&key, partial_location, &partial.source) {
                    Ok(location) => {
                        locations_map.insert(key, location);
                    }
                    Err(errors) => {
                        diagnostics.extend(
                            errors
                                .into_iter()
                                .map(|e| Diagnostic::Error(Error::Validation(e))),
                        );
                    }
                }
            }

            LocationsConfig {
                default: partial_locations.default.map(|s| s.into_inner()),
                locations: locations_map,
            }
        } else {
            LocationsConfig::default()
        };

        // Convert integrations config
        let integrations = partial
            .integrations
            .map(|_| IntegrationsConfig {})
            .unwrap_or_default();

        let config = Config {
            logging,
            locations,
            integrations,
        };

        // Validate cross-field constraints
        if let Err(validation_error) = config.validate() {
            diagnostics.push(Diagnostic::Error(Error::Validation(ValidationError {
                field_path: "locations.default".to_string(),
                message: validation_error,
                span: None,
                source: None,
            })));
        }

        // Check if there are any errors (not just warnings)
        let has_errors = diagnostics.iter().any(|d| d.is_error());

        if has_errors {
            Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format_diagnostics(&diagnostics),
            )))
        } else {
            Ok((config, diagnostics))
        }
    }

    /// Validate a partial location and convert it to a complete Location
    fn validate_location(
        name: &str,
        partial: PartialLocation,
        source: &Option<SourceInfo>,
    ) -> Result<Location, Vec<ValidationError>> {
        let mut errors = Vec::new();

        // Latitude is required
        let latitude = if let Some(lat) = partial.latitude {
            lat
        } else {
            errors.push(ValidationError {
                field_path: format!("locations.{}.latitude", name),
                message: "latitude is required".to_string(),
                span: None,
                source: source.clone(),
            });
            0.0 // Default for error recovery
        };

        // Longitude is required
        let longitude = if let Some(lon) = partial.longitude {
            lon
        } else {
            errors.push(ValidationError {
                field_path: format!("locations.{}.longitude", name),
                message: "longitude is required".to_string(),
                span: None,
                source: source.clone(),
            });
            0.0 // Default for error recovery
        };

        let elevation_m = partial.elevation_m;
        let timezone = partial.timezone;

        if errors.is_empty() {
            Ok(Location {
                latitude,
                longitude,
                elevation_m,
                timezone,
            })
        } else {
            Err(errors)
        }
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
    use std::fs;
    use std::io::Write;

    // All tests now use Config::from_files() with actual file I/O
    // This ensures we test the real loading path

    #[test]
    fn test_merge_non_overlapping_configs() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_merge");
        fs::create_dir_all(&temp_dir).unwrap();

        let base_path = temp_dir.join("base.toml");
        let mut base_file = fs::File::create(&base_path).unwrap();
        write!(
            base_file,
            r#"
[logging]
level = "info"

[locations.home]
latitude = 59.9139
longitude = 10.7522
"#
        )
        .unwrap();

        let extra_path = temp_dir.join("extra.toml");
        let mut extra_file = fs::File::create(&extra_path).unwrap();
        write!(
            extra_file,
            r#"
[logging.overrides]
"hearthd::api" = "debug"

[locations.work]
latitude = 60.0
longitude = 11.0
"#
        )
        .unwrap();

        let result = Config::from_files(&[base_path.clone(), extra_path.clone()]);
        assert!(result.is_ok(), "Config loading failed: {:?}", result.err());

        let (config, diagnostics) = result.unwrap();
        assert_eq!(diagnostics.len(), 0, "Expected no diagnostics");
        assert_eq!(config.logging.level, LogLevel::Info);
        assert_eq!(config.logging.overrides.len(), 1);
        assert_eq!(
            config.logging.overrides.get("hearthd::api"),
            Some(&LogLevel::Debug)
        );
        assert_eq!(config.locations.locations.len(), 2);
        assert!(config.locations.locations.contains_key("home"));
        assert!(config.locations.locations.contains_key("work"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_conflict_detection() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_conflict");
        fs::create_dir_all(&temp_dir).unwrap();

        let base_path = temp_dir.join("base.toml");
        let mut base_file = fs::File::create(&base_path).unwrap();
        write!(
            base_file,
            r#"
[logging]
level = "info"
"#
        )
        .unwrap();

        let conflict_path = temp_dir.join("conflict.toml");
        let mut conflict_file = fs::File::create(&conflict_path).unwrap();
        write!(
            conflict_file,
            r#"
[logging]
level = "debug"
"#
        )
        .unwrap();

        let result = Config::from_files(&[base_path.clone(), conflict_path.clone()]);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Merge conflict"));
        assert!(err_msg.contains("logging.level"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_import_resolution() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_imports");
        fs::create_dir_all(&temp_dir).unwrap();

        let base_path = temp_dir.join("base.toml");
        let mut base_file = fs::File::create(&base_path).unwrap();
        write!(
            base_file,
            r#"
[logging]
level = "info"
"#
        )
        .unwrap();

        let main_path = temp_dir.join("main.toml");
        let mut main_file = fs::File::create(&main_path).unwrap();
        write!(
            main_file,
            r#"
imports = ["base.toml"]

[locations.home]
latitude = 59.9139
longitude = 10.7522
"#
        )
        .unwrap();

        let result = Config::from_files(&[main_path.clone()]);
        assert!(result.is_ok());

        let (config, _diagnostics) = result.unwrap();
        assert_eq!(config.logging.level, LogLevel::Info);
        assert_eq!(config.locations.locations.len(), 1);

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_import_cycle_detection() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_cycle");
        fs::create_dir_all(&temp_dir).unwrap();

        let a_path = temp_dir.join("a.toml");
        let mut a_file = fs::File::create(&a_path).unwrap();
        write!(
            a_file,
            r#"
imports = ["b.toml"]

[logging]
level = "info"
"#
        )
        .unwrap();

        let b_path = temp_dir.join("b.toml");
        let mut b_file = fs::File::create(&b_path).unwrap();
        write!(
            b_file,
            r#"
imports = ["a.toml"]

[locations]
"#
        )
        .unwrap();

        let result = Config::from_files(&[a_path.clone()]);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("cycle") || err_msg.contains("Import"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_multiple_conflicts_reported() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_multi_conflict");
        fs::create_dir_all(&temp_dir).unwrap();

        let base_path = temp_dir.join("base.toml");
        let mut base_file = fs::File::create(&base_path).unwrap();
        write!(
            base_file,
            r#"
[logging]
level = "info"

[logging.overrides]
"target1" = "trace"

[locations]
default = "home"
"#
        )
        .unwrap();

        let conflict_path = temp_dir.join("conflict.toml");
        let mut conflict_file = fs::File::create(&conflict_path).unwrap();
        write!(
            conflict_file,
            r#"
[logging]
level = "debug"

[logging.overrides]
"target1" = "error"

[locations]
default = "work"
"#
        )
        .unwrap();

        let result = Config::from_files(&[base_path.clone(), conflict_path.clone()]);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        // Should report all 3 conflicts
        assert!(err_msg.contains("logging.level"));
        assert!(err_msg.contains("logging.overrides.target1"));
        assert!(err_msg.contains("locations.default"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_relative_import_paths() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_relative");
        let subdir = temp_dir.join("configs");
        fs::create_dir_all(&subdir).unwrap();

        let base_path = subdir.join("base.toml");
        let mut base_file = fs::File::create(&base_path).unwrap();
        write!(
            base_file,
            r#"
[logging]
level = "info"
"#
        )
        .unwrap();

        let main_path = temp_dir.join("main.toml");
        let mut main_file = fs::File::create(&main_path).unwrap();
        write!(
            main_file,
            r#"
imports = ["configs/base.toml"]

[locations.home]
latitude = 59.9139
longitude = 10.7522
"#
        )
        .unwrap();

        let result = Config::from_files(&[main_path.clone()]);
        assert!(result.is_ok());

        let (config, _diagnostics) = result.unwrap();
        assert_eq!(config.logging.level, LogLevel::Info);

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_empty_config_file() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_empty");
        fs::create_dir_all(&temp_dir).unwrap();

        let empty_path = temp_dir.join("empty.toml");
        let _empty_file = fs::File::create(&empty_path).unwrap();
        // File is completely empty

        let result = Config::from_files(&[empty_path.clone()]);

        // Empty file should parse successfully but emit warning
        assert!(result.is_ok(), "Empty config should parse successfully");

        let (config, diagnostics) = result.unwrap();
        assert_eq!(diagnostics.len(), 1, "Expected 1 warning for empty config");
        assert!(diagnostics[0].is_warning(), "Expected a warning");

        assert_eq!(config.logging.level, LogLevel::Info); // Default
        assert_eq!(config.locations.locations.len(), 0);

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_minimal_location_only_config() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_minimal");
        fs::create_dir_all(&temp_dir).unwrap();

        let minimal_path = temp_dir.join("minimal.toml");
        let mut minimal_file = fs::File::create(&minimal_path).unwrap();
        write!(
            minimal_file,
            "[locations.home]\nlatitude = 59.9139\nlongitude = 10.7522"
        )
        .unwrap();

        let result = Config::from_files(&[minimal_path.clone()]);
        assert!(result.is_ok(), "Minimal config should parse successfully: {:?}", result.err());

        let (config, diagnostics) = result.unwrap();
        assert_eq!(diagnostics.len(), 0, "Expected no diagnostics for valid config");

        // Logging should use defaults
        assert_eq!(config.logging.level, LogLevel::Info);
        assert_eq!(config.logging.overrides.len(), 0);

        // Location should be present
        assert_eq!(config.locations.locations.len(), 1);
        assert!(config.locations.locations.contains_key("home"));

        let home = config.locations.locations.get("home").unwrap();
        assert_eq!(home.latitude, 59.9139);
        assert_eq!(home.longitude, 10.7522);
        assert_eq!(home.elevation_m, None);
        assert_eq!(home.timezone, None);

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_missing_file_error() {
        let missing_path = PathBuf::from("/nonexistent/config.toml");

        let result = Config::from_files(&[missing_path.clone()]);
        assert!(result.is_err(), "Should fail when file doesn't exist");

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to read"), "Error should mention read failure");
        assert!(err_msg.contains("/nonexistent/config.toml"), "Error should include file path");
    }
}
