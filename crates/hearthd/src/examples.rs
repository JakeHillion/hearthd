//! # Configuration Error and Diagnostic Examples
//!
//! This module contains doc tests that demonstrate the various diagnostics and
//! error messages that can be produced by the hearthd configuration system.
//! These examples use `cargo-insta` to snapshot the formatted output, making it
//! easy to review changes to error formatting.
//!
//! ## Merge Conflicts
//!
//! When the same field is defined in multiple configuration files, the first
//! value wins and a merge conflict error is reported:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! // First config defines logging level as "info"
//! let base_path = temp_dir.path().join("base.toml");
//! let mut base_file = fs::File::create(&base_path).unwrap();
//! write!(
//!     base_file,
//!     r#"
//! [logging]
//! level = "info"
//! "#
//! ).unwrap();
//!
//! // Second config tries to define logging level as "debug"
//! let override_path = temp_dir.path().join("override.toml");
//! let mut override_file = fs::File::create(&override_path).unwrap();
//! write!(
//!     override_file,
//!     r#"
//! [logging]
//! level = "debug"
//! "#
//! ).unwrap();
//!
//! // This will produce a merge conflict error
//! let result = Config::from_files(&[base_path.clone(), override_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! // Snapshot the error output (file paths are normalized for stability)
//! let error_str = error.to_string()
//!     .replace(&base_path.display().to_string(), "base.toml")
//!     .replace(&override_path.display().to_string(), "override.toml");
//! insta::assert_snapshot!("merge_conflict", error_str);
//! ```
//!
//! ## Validation Errors
//!
//! Incomplete location definitions (missing required fields) produce validation errors:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! // Location missing longitude
//! let config_path = temp_dir.path().join("config.toml");
//! let mut config_file = fs::File::create(&config_path).unwrap();
//! write!(
//!     config_file,
//!     r#"
//! [locations.home]
//! latitude = 59.9139
//! "#
//! ).unwrap();
//!
//! let result = Config::from_files(&[config_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string()
//!     .replace(&config_path.display().to_string(), "config.toml");
//! insta::assert_snapshot!("validation_error_missing_longitude", error_str);
//!
//! ```
//!
//! ## Empty Config Warning
//!
//! Empty configuration files produce a warning but don't prevent loading:
//!
//! ```
//! use hearthd::{Config, format_diagnostics};
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let empty_path = temp_dir.path().join("empty.toml");
//! let mut empty_file = fs::File::create(&empty_path).unwrap();
//! write!(empty_file, "# Just a comment\n").unwrap();
//!
//! let result = Config::from_files(&[empty_path.clone()]);
//! assert!(result.is_ok());
//!
//! let (_config, diagnostics) = result.unwrap();
//! let output = format_diagnostics(&diagnostics.0[..]);
//! println!("{}", output);
//!
//! let output_str = output
//!     .replace(&empty_path.display().to_string(), "empty.toml");
//! insta::assert_snapshot!("empty_config_warning", output_str);
//!
//! ```
//!
//! ## Multiple Conflicts
//!
//! Multiple fields can conflict simultaneously, each producing its own diagnostic:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let config1_path = temp_dir.path().join("config1.toml");
//! let mut config1 = fs::File::create(&config1_path).unwrap();
//! write!(
//!     config1,
//!     r#"
//! [logging]
//! level = "info"
//!
//! [locations]
//! default = "home"
//! "#
//! ).unwrap();
//!
//! let config2_path = temp_dir.path().join("config2.toml");
//! let mut config2 = fs::File::create(&config2_path).unwrap();
//! write!(
//!     config2,
//!     r#"
//! [logging]
//! level = "debug"
//!
//! [locations]
//! default = "work"
//! "#
//! ).unwrap();
//!
//! let result = Config::from_files(&[config1_path.clone(), config2_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string()
//!     .replace(&config1_path.display().to_string(), "config1.toml")
//!     .replace(&config2_path.display().to_string(), "config2.toml");
//! insta::assert_snapshot!("multiple_conflicts", error_str);
//!
//! ```
//!
//! ## Import Conflicts
//!
//! Conflicts can occur across imported files:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let imported_path = temp_dir.path().join("imported.toml");
//! let mut imported = fs::File::create(&imported_path).unwrap();
//! write!(
//!     imported,
//!     r#"
//! [logging]
//! level = "debug"
//! "#
//! ).unwrap();
//!
//! let main_path = temp_dir.path().join("main.toml");
//! let mut main_file = fs::File::create(&main_path).unwrap();
//! write!(
//!     main_file,
//!     r#"
//! imports = ["{}"]
//!
//! [logging]
//! level = "info"
//! "#,
//!     imported_path.display()
//! ).unwrap();
//!
//! let result = Config::from_files(&[main_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string()
//!     .replace(&main_path.display().to_string(), "main.toml")
//!     .replace(&imported_path.display().to_string(), "imported.toml");
//! insta::assert_snapshot!("import_conflict", error_str);
//!
//! ```
//!
//! ## Complex Scenario
//!
//! Complex configurations with multiple files, imports, and conflicts:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! // Base config
//! let base_path = temp_dir.path().join("base.toml");
//! let mut base = fs::File::create(&base_path).unwrap();
//! write!(
//!     base,
//!     r#"
//! [logging]
//! level = "info"
//!
//! [locations.home]
//! latitude = 59.9139
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! // Override config with conflicts
//! let override_path = temp_dir.path().join("override.toml");
//! let mut override_file = fs::File::create(&override_path).unwrap();
//! write!(
//!     override_file,
//!     r#"
//! imports = ["{}"]
//!
//! [logging]
//! level = "debug"
//!
//! [locations.home]
//! latitude = 60.0
//! "#,
//!     base_path.display()
//! ).unwrap();
//!
//! let result = Config::from_files(&[override_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string()
//!     .replace(&base_path.display().to_string(), "base.toml")
//!     .replace(&override_path.display().to_string(), "override.toml");
//! insta::assert_snapshot!("complex_scenario", error_str);
//!
//! ```
//!
//! ## Successful Config with Warnings
//!
//! Configurations can load successfully while still producing warnings:
//!
//! ```
//! use hearthd::{Config, format_diagnostics};
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let config_path = temp_dir.path().join("config.toml");
//! let mut config_file = fs::File::create(&config_path).unwrap();
//! write!(
//!     config_file,
//!     r#"
//! [locations.home]
//! latitude = 59.9139
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! let empty_path = temp_dir.path().join("empty.toml");
//! let mut empty_file = fs::File::create(&empty_path).unwrap();
//! write!(empty_file, "# Empty\n").unwrap();
//!
//! let result = Config::from_files(&[config_path.clone(), empty_path.clone()]);
//! assert!(result.is_ok());
//!
//! let (config, diagnostics) = result.unwrap();
//! assert_eq!(config.locations.locations.get("home").unwrap().latitude, 59.9139);
//!
//! let output = format_diagnostics(&diagnostics.0[..]);
//! println!("{}", output);
//!
//! let output_str = output
//!     .replace(&empty_path.display().to_string(), "empty.toml");
//! insta::assert_snapshot!("success_with_warnings", output_str);
//!
//! ```
//!
//! ## Default Location Validation
//!
//! The default location must reference an existing location:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let config_path = temp_dir.path().join("config.toml");
//! let mut config_file = fs::File::create(&config_path).unwrap();
//! write!(
//!     config_file,
//!     r#"
//! [locations]
//! default = "nonexistent"
//!
//! [locations.home]
//! latitude = 59.9139
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! let result = Config::from_files(&[config_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string()
//!     .replace(&config_path.display().to_string(), "config.toml");
//! insta::assert_snapshot!("default_location_validation", error_str);
//!
//! ```
//!
//! ## Location Field Conflicts
//!
//! When the same location field appears in multiple configs, it conflicts:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let config1_path = temp_dir.path().join("config1.toml");
//! let mut config1 = fs::File::create(&config1_path).unwrap();
//! write!(
//!     config1,
//!     r#"
//! [locations.home]
//! latitude = 59.9139
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! let config2_path = temp_dir.path().join("config2.toml");
//! let mut config2 = fs::File::create(&config2_path).unwrap();
//! write!(
//!     config2,
//!     r#"
//! [locations.home]
//! latitude = 60.0
//! "#
//! ).unwrap();
//!
//! let result = Config::from_files(&[config1_path.clone(), config2_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string()
//!     .replace(&config1_path.display().to_string(), "config1.toml")
//!     .replace(&config2_path.display().to_string(), "config2.toml");
//! insta::assert_snapshot!("location_field_conflict", error_str);
//!
//! ```
//!
//! ## Import Nonexistent File
//!
//! Importing a file that doesn't exist produces an error:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let config_path = temp_dir.path().join("config.toml");
//! let mut config_file = fs::File::create(&config_path).unwrap();
//! write!(
//!     config_file,
//!     r#"
//! imports = ["/tmp/doesntexist.toml"]
//!
//! [locations.home]
//! latitude = 59.9139
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! let result = Config::from_files(&[config_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string();
//! // Just check that it contains the expected parts
//! assert!(error_str.contains("Error"));
//! assert!(error_str.contains("/tmp/doesntexist.toml"));
//!
//! ```
//!
//! ## Invalid TOML
//!
//! Invalid TOML syntax produces a parse error:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let config_path = temp_dir.path().join("config.toml");
//! let mut config_file = fs::File::create(&config_path).unwrap();
//! write!(
//!     config_file,
//!     r#"
//! [locations.home
//! latitude = 59.9139
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! let result = Config::from_files(&[config_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string();
//! // Just check that it contains the expected parts
//! assert!(error_str.contains("Error"));
//! assert!(error_str.contains("parse") || error_str.contains("TOML"));
//!
//! ```
//!
//! ## Valid Split Config
//!
//! Non-conflicting fields from the same location can be split across files:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let config1_path = temp_dir.path().join("config1.toml");
//! let mut config1 = fs::File::create(&config1_path).unwrap();
//! write!(
//!     config1,
//!     r#"
//! [locations.home]
//! latitude = 59.9139
//! "#
//! ).unwrap();
//!
//! let config2_path = temp_dir.path().join("config2.toml");
//! let mut config2 = fs::File::create(&config2_path).unwrap();
//! write!(
//!     config2,
//!     r#"
//! [locations.home]
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! let result = Config::from_files(&[config1_path, config2_path]);
//! assert!(result.is_ok());
//!
//! let (config, _diagnostics) = result.unwrap();
//! let home = config.locations.locations.get("home").unwrap();
//! assert_eq!(home.latitude, 59.9139);
//! assert_eq!(home.longitude, 10.7522);
//! println!("Successfully merged split config!");
//!
//! ```
//!
//! ## Field Conflict - Same Value
//!
//! Even when values are identical, defining the same field twice is a conflict:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let config1_path = temp_dir.path().join("config1.toml");
//! let mut config1 = fs::File::create(&config1_path).unwrap();
//! write!(
//!     config1,
//!     r#"
//! [locations.home]
//! latitude = 59.9139
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! let config2_path = temp_dir.path().join("config2.toml");
//! let mut config2 = fs::File::create(&config2_path).unwrap();
//! write!(
//!     config2,
//!     r#"
//! [locations.home]
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! let result = Config::from_files(&[config1_path.clone(), config2_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string()
//!     .replace(&config1_path.display().to_string(), "config1.toml")
//!     .replace(&config2_path.display().to_string(), "config2.toml");
//! insta::assert_snapshot!("field_conflict_same_value", error_str);
//!
//! ```
//!
//! ## Field Conflict - Different Values
//!
//! Defining the same field with different values is also a conflict:
//!
//! ```
//! use hearthd::Config;
//! use std::fs;
//! use std::io::Write;
//!
//! let temp_dir = tempfile::tempdir().unwrap();
//!
//! let config1_path = temp_dir.path().join("config1.toml");
//! let mut config1 = fs::File::create(&config1_path).unwrap();
//! write!(
//!     config1,
//!     r#"
//! [locations.home]
//! latitude = 59.9139
//! longitude = 10.7522
//! "#
//! ).unwrap();
//!
//! let config2_path = temp_dir.path().join("config2.toml");
//! let mut config2 = fs::File::create(&config2_path).unwrap();
//! write!(
//!     config2,
//!     r#"
//! [locations.home]
//! longitude = 11.0
//! "#
//! ).unwrap();
//!
//! let result = Config::from_files(&[config1_path.clone(), config2_path.clone()]);
//! assert!(result.is_err());
//!
//! let error = result.unwrap_err();
//! println!("{}", error);
//!
//! let error_str = error.to_string()
//!     .replace(&config1_path.display().to_string(), "config1.toml")
//!     .replace(&config2_path.display().to_string(), "config2.toml");
//! insta::assert_snapshot!("field_conflict_different_values", error_str);
//!
//! ```
