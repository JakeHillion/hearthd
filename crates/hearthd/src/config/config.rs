use std::collections::HashMap;
use std::path::PathBuf;

use hearthd_config::Diagnostic;
use hearthd_config::Diagnostics;
use hearthd_config::Error;
use hearthd_config::MergeableConfig;
use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use hearthd_config::ValidationError;
use serde::Deserialize;
use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, Default, TryFromPartial, MergeableConfig)]
pub struct Config {
    pub logging: LoggingConfig,
    pub locations: LocationsConfig,
    pub http: HttpConfig,
    pub integrations: IntegrationsConfig,
    pub automations: AutomationsConfig,
}

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

#[derive(Debug, Default, Deserialize, TryFromPartial, SubConfig)]
pub struct LoggingConfig {
    /// Log level: trace, debug, info, warn, error
    pub level: LogLevel,

    pub overrides: HashMap<String, LogLevel>,
}

#[derive(Debug, Default, Deserialize, TryFromPartial, SubConfig)]
pub struct LocationsConfig {
    /// The default location to use
    pub default: Option<String>,

    /// Named locations with their configurations
    #[serde(flatten)]
    pub locations: HashMap<String, Location>,
}

#[derive(Debug, Clone, Default, Deserialize, TryFromPartial, SubConfig)]
// Required: toml deserializer cannot wrap fields in Spanned when the parent HashMap
// is flattened. Without this, deserialization fails with "expected a spanned value".
#[config(no_span)]
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

#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct HttpConfig {
    /// Listen address for the HTTP API server
    pub listen: String,

    /// Port for the HTTP API server
    pub port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1".to_string(),
            port: 8565,
        }
    }
}

#[derive(Debug, Default, Deserialize, TryFromPartial, SubConfig)]
pub struct IntegrationsConfig {
    #[cfg(feature = "integration_mqtt")]
    pub mqtt: Option<crate::integrations::mqtt::MqttConfig>,
}

#[derive(Debug, Default, Deserialize, TryFromPartial, SubConfig)]
pub struct AutomationsConfig {
    /// Named automations (key is name, value contains file path)
    #[serde(flatten)]
    pub automations: HashMap<String, AutomationEntry>,
}

#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
pub struct AutomationEntry {
    /// Path to the .hda file
    pub file: String,
}

impl Config {
    /// Load configuration from multiple TOML files with import resolution
    pub fn from_files(paths: &[PathBuf]) -> Result<(Self, Diagnostics), Diagnostics> {
        // Use generated load and merge
        let configs = PartialConfig::load_with_imports(paths)
            .map_err(|e| Diagnostics(vec![Diagnostic::Error(hearthd_config::Error::Load(e))]))?;

        let (partial, mut diagnostics) = PartialConfig::merge(configs);

        // Convert from partial config using TryFromPartial
        let config = match Config::try_from_partial(partial) {
            Ok(cfg) => cfg,
            Err(errs) => {
                diagnostics.extend(errs);
                Config::default()
            }
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

        let has_errors = diagnostics.iter().any(|d| d.is_error());

        if has_errors {
            Err(Diagnostics(diagnostics))
        } else {
            Ok((config, Diagnostics(diagnostics)))
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
    use std::fs;
    use std::io::Write;

    use super::*;

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
        assert_eq!(diagnostics.0.len(), 0, "Expected no diagnostics");
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

        let result = Config::from_files(std::slice::from_ref(&main_path));
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

        let result = Config::from_files(std::slice::from_ref(&a_path));
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

        let result = Config::from_files(std::slice::from_ref(&main_path));
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

        let result = Config::from_files(std::slice::from_ref(&empty_path));

        // Empty file should parse successfully but emit warning
        assert!(result.is_ok(), "Empty config should parse successfully");

        let (config, diagnostics) = result.unwrap();
        assert_eq!(
            diagnostics.0.len(),
            1,
            "Expected 1 warning for empty config"
        );
        assert!(diagnostics.0[0].is_warning(), "Expected a warning");

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

        let result = Config::from_files(std::slice::from_ref(&minimal_path));
        assert!(
            result.is_ok(),
            "Minimal config should parse successfully: {:?}",
            result.err()
        );

        let (config, diagnostics) = result.unwrap();
        assert_eq!(
            diagnostics.0.len(),
            0,
            "Expected no diagnostics for valid config"
        );

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

        let result = Config::from_files(std::slice::from_ref(&missing_path));
        assert!(result.is_err(), "Should fail when file doesn't exist");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Failed to read"),
            "Error should mention read failure"
        );
        assert!(
            err_msg.contains("/nonexistent/config.toml"),
            "Error should include file path"
        );
    }

    #[test]
    fn test_validation_error_missing_latitude() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_missing_latitude");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("config.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(
            config_file,
            r#"
[locations.home]
longitude = 10.7522
"#
        )
        .unwrap();

        let result = Config::from_files(&[config_path]);
        assert!(result.is_err(), "Should fail when latitude is missing");

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("latitude"));
        assert!(err_msg.contains("required"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_validation_error_missing_longitude() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_missing_longitude");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("config.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(
            config_file,
            r#"
[locations.home]
latitude = 59.9139
"#
        )
        .unwrap();

        let result = Config::from_files(&[config_path]);
        assert!(result.is_err(), "Should fail when longitude is missing");

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("longitude"));
        assert!(err_msg.contains("required"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_validation_error_invalid_default_location() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_invalid_default");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("config.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(
            config_file,
            r#"
[logging]
level = "info"

[locations]
default = "nonexistent"

[locations.home]
latitude = 59.9139
longitude = 10.7522
"#
        )
        .unwrap();

        let result = Config::from_files(&[config_path]);
        assert!(
            result.is_err(),
            "Should fail when default location doesn't exist"
        );

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("nonexistent"));
        assert!(err_msg.contains("not found"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_logging_overrides() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_logging_overrides");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("config.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(
            config_file,
            r#"
[logging]
level = "info"

[logging.overrides]
"hearthd::api" = "debug"
"hearthd::db" = "trace"
"other::module" = "warn"
"#
        )
        .unwrap();

        let result = Config::from_files(&[config_path]);
        assert!(result.is_ok(), "Config with overrides should load");

        let (config, diagnostics) = result.unwrap();
        assert_eq!(diagnostics.0.len(), 0);
        assert_eq!(config.logging.level, LogLevel::Info);
        assert_eq!(config.logging.overrides.len(), 3);
        assert_eq!(
            config.logging.overrides.get("hearthd::api"),
            Some(&LogLevel::Debug)
        );
        assert_eq!(
            config.logging.overrides.get("hearthd::db"),
            Some(&LogLevel::Trace)
        );
        assert_eq!(
            config.logging.overrides.get("other::module"),
            Some(&LogLevel::Warn)
        );

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_location_with_all_fields() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_location_full");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("config.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(
            config_file,
            r#"
[locations.oslo]
latitude = 59.9139
longitude = 10.7522
elevation_m = 23.0
timezone = "Europe/Oslo"
"#
        )
        .unwrap();

        let result = Config::from_files(&[config_path]);
        assert!(result.is_ok());

        let (config, _) = result.unwrap();
        let oslo = config.locations.locations.get("oslo").unwrap();
        assert_eq!(oslo.latitude, 59.9139);
        assert_eq!(oslo.longitude, 10.7522);
        assert_eq!(oslo.elevation_m, Some(23.0));
        assert_eq!(oslo.timezone, Some("Europe/Oslo".to_string()));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_multiple_locations() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_multiple_locations");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("config.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(
            config_file,
            r#"
[locations]
default = "home"

[locations.home]
latitude = 59.9139
longitude = 10.7522
elevation_m = 10.0
timezone = "Europe/Oslo"

[locations.work]
latitude = 59.9110
longitude = 10.7579

[locations.cabin]
latitude = 60.5
longitude = 11.0
elevation_m = 450.0
"#
        )
        .unwrap();

        let result = Config::from_files(&[config_path]);
        assert!(result.is_ok());

        let (config, diagnostics) = result.unwrap();
        assert_eq!(diagnostics.0.len(), 0);
        assert_eq!(config.locations.default, Some("home".to_string()));
        assert_eq!(config.locations.locations.len(), 3);
        assert!(config.locations.locations.contains_key("home"));
        assert!(config.locations.locations.contains_key("work"));
        assert!(config.locations.locations.contains_key("cabin"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_all_log_levels() {
        for (level_str, level_enum) in [
            ("trace", LogLevel::Trace),
            ("debug", LogLevel::Debug),
            ("info", LogLevel::Info),
            ("warn", LogLevel::Warn),
            ("error", LogLevel::Error),
        ] {
            let temp_dir =
                std::env::temp_dir().join(format!("hearthd_test_loglevel_{}", level_str));
            fs::create_dir_all(&temp_dir).unwrap();

            let config_path = temp_dir.join("config.toml");
            let mut config_file = fs::File::create(&config_path).unwrap();
            write!(
                config_file,
                r#"
[logging]
level = "{}"
"#,
                level_str
            )
            .unwrap();

            let result = Config::from_files(&[config_path]);
            assert!(result.is_ok(), "Log level {} should be valid", level_str);

            let (config, _) = result.unwrap();
            assert_eq!(config.logging.level, level_enum);

            fs::remove_dir_all(&temp_dir).ok();
        }
    }

    #[test]
    fn test_complex_multi_file_merge() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_complex_merge");
        fs::create_dir_all(&temp_dir).unwrap();

        // Base config with logging
        let base_path = temp_dir.join("base.toml");
        let mut base_file = fs::File::create(&base_path).unwrap();
        write!(
            base_file,
            r#"
[logging]
level = "info"

[logging.overrides]
"hearthd" = "debug"
"#
        )
        .unwrap();

        // Locations config
        let locations_path = temp_dir.join("locations.toml");
        let mut locations_file = fs::File::create(&locations_path).unwrap();
        write!(
            locations_file,
            r#"
[locations]
default = "home"

[locations.home]
latitude = 59.9139
longitude = 10.7522
"#
        )
        .unwrap();

        // Additional overrides
        let overrides_path = temp_dir.join("overrides.toml");
        let mut overrides_file = fs::File::create(&overrides_path).unwrap();
        write!(
            overrides_file,
            r#"
[logging.overrides]
"external::lib" = "warn"

[locations.work]
latitude = 60.0
longitude = 11.0
"#
        )
        .unwrap();

        let result = Config::from_files(&[
            base_path.clone(),
            locations_path.clone(),
            overrides_path.clone(),
        ]);
        assert!(result.is_ok());

        let (config, diagnostics) = result.unwrap();
        assert_eq!(diagnostics.0.len(), 0);
        assert_eq!(config.logging.level, LogLevel::Info);
        assert_eq!(config.logging.overrides.len(), 2);
        assert_eq!(config.locations.default, Some("home".to_string()));
        assert_eq!(config.locations.locations.len(), 2);

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_diagnostics_output_format() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_diagnostics_format");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("config.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(config_file, "").unwrap(); // Empty file

        let result = Config::from_files(&[config_path]);
        assert!(result.is_ok(), "Empty config should succeed with warning");

        let (_config, diagnostics) = result.unwrap();
        assert_eq!(diagnostics.0.len(), 1);
        assert!(diagnostics.0[0].is_warning());

        // Test that format_diagnostics produces output
        let output = hearthd_config::format_diagnostics(&diagnostics.0);
        assert!(!output.is_empty());
        assert!(output.contains("Warning"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_nested_field() {
        use hearthd_config::MergeableConfig;
        use hearthd_config::SubConfig;
        use serde::Deserialize;
        use tempfile::TempDir;

        #[derive(MergeableConfig, Deserialize, Debug)]
        #[allow(dead_code)]
        struct TestConfig {
            name: String,
            details: Details,
        }

        #[derive(SubConfig, Deserialize, Debug, Clone, PartialEq)]
        #[allow(dead_code)]
        struct Details {
            description: String,
            count: u32,
        }

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        fs::write(
            &config_path,
            r#"
            name = "test"

            [details]
            description = "test description"
            count = 42
            "#,
        )
        .unwrap();

        let configs = PartialTestConfig::load_with_imports(&[config_path]).unwrap();
        let (merged, diagnostics) = PartialTestConfig::merge(configs);

        assert_eq!(diagnostics.len(), 0);
        assert_eq!(merged.name.unwrap().into_inner(), "test");

        let details = merged.details.unwrap();
        assert_eq!(
            details.description.unwrap().into_inner(),
            "test description"
        );
        assert_eq!(details.count.unwrap().into_inner(), 42);

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_use_spans_false() {
        use hearthd_config::MergeableConfig;
        use serde::Deserialize;
        use tempfile::TempDir;

        #[derive(MergeableConfig, Deserialize, Debug)]
        #[config(no_span)]
        #[allow(dead_code)]
        struct TestConfig {
            port: u16,
            host: String,
        }

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        fs::write(
            &config_path,
            r#"
            port = 8080
            host = "localhost"
            "#,
        )
        .unwrap();

        let configs = PartialTestConfig::load_with_imports(&[config_path]).unwrap();
        let (merged, diagnostics) = PartialTestConfig::merge(configs);

        assert_eq!(diagnostics.len(), 0);
        assert_eq!(merged.port.unwrap(), 8080);
        assert_eq!(merged.host.unwrap(), "localhost");

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_deeply_nested_configs() {
        use hearthd_config::MergeableConfig;
        use hearthd_config::SubConfig;
        use serde::Deserialize;
        use tempfile::TempDir;

        #[derive(MergeableConfig, Deserialize, Debug)]
        #[allow(dead_code)]
        struct TestConfig {
            level1: Level1,
        }

        #[derive(SubConfig, Deserialize, Debug, Clone, PartialEq)]
        #[allow(dead_code)]
        struct Level1 {
            name: String,
            level2: Level2,
        }

        #[derive(SubConfig, Deserialize, Debug, Clone, PartialEq)]
        #[allow(dead_code)]
        struct Level2 {
            value: u32,
            level3: Level3,
        }

        #[derive(SubConfig, Deserialize, Debug, Clone, PartialEq)]
        #[allow(dead_code)]
        struct Level3 {
            data: String,
        }

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        fs::write(
            &config_path,
            r#"
            [level1]
            name = "first"

            [level1.level2]
            value = 100

            [level1.level2.level3]
            data = "deep value"
            "#,
        )
        .unwrap();

        let configs = PartialTestConfig::load_with_imports(&[config_path]).unwrap();
        let (merged, diagnostics) = PartialTestConfig::merge(configs);

        assert_eq!(diagnostics.len(), 0);

        let level1 = merged.level1.unwrap();
        assert_eq!(level1.name.unwrap().into_inner(), "first");

        let level2 = level1.level2.unwrap();
        assert_eq!(level2.value.unwrap().into_inner(), 100);

        let level3 = level2.level3.unwrap();
        assert_eq!(level3.data.unwrap().into_inner(), "deep value");

        fs::remove_dir_all(&temp_dir).ok();
    }
}
