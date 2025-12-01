use hearthd_config::Diagnostic;
use hearthd_config::TryFromPartial as _;
use hearthd_config_derive::SubConfig;
use hearthd_config_derive::TryFromPartial;
use serde::Deserialize;

// Helper functions for default values
fn default_port() -> u16 {
    8080
}

fn default_host() -> String {
    "localhost".to_string()
}

fn default_timeout() -> u64 {
    30
}

fn default_server_config() -> ServerConfig {
    ServerConfig {
        host: "default-host".to_string(),
        port: 9090,
        timeout: 60,
    }
}

// Test simple types with defaults
#[derive(Debug, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
struct SimpleConfig {
    #[config(default = "default_port")]
    port: u16,

    #[config(default = "default_host")]
    host: String,

    #[config(default = "default_timeout")]
    timeout: u64,

    // Required field without default (should still error if missing)
    required_field: String,
}

#[test]
fn test_simple_defaults_all_missing() {
    let toml = r#"
        required_field = "test"
    "#;

    let partial: PartialSimpleConfig = toml::from_str(toml).unwrap();
    let result = SimpleConfig::try_from_partial(partial);

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.port, 8080);
    assert_eq!(config.host, "localhost");
    assert_eq!(config.timeout, 30);
    assert_eq!(config.required_field, "test");
}

#[test]
fn test_simple_defaults_some_provided() {
    let toml = r#"
        port = 3000
        required_field = "test"
    "#;

    let partial: PartialSimpleConfig = toml::from_str(toml).unwrap();
    let result = SimpleConfig::try_from_partial(partial);

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.port, 3000); // Provided value
    assert_eq!(config.host, "localhost"); // Default
    assert_eq!(config.timeout, 30); // Default
    assert_eq!(config.required_field, "test");
}

#[test]
fn test_simple_defaults_all_provided() {
    let toml = r#"
        port = 3000
        host = "example.com"
        timeout = 120
        required_field = "test"
    "#;

    let partial: PartialSimpleConfig = toml::from_str(toml).unwrap();
    let result = SimpleConfig::try_from_partial(partial);

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.port, 3000);
    assert_eq!(config.host, "example.com");
    assert_eq!(config.timeout, 120);
    assert_eq!(config.required_field, "test");
}

#[test]
fn test_required_field_missing_generates_error() {
    let toml = r#"
        port = 3000
    "#;

    let partial: PartialSimpleConfig = toml::from_str(toml).unwrap();
    let result = SimpleConfig::try_from_partial(partial);

    // Should error because required_field is missing and has no default
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    match &errors[0] {
        Diagnostic::Error(hearthd_config::Error::Validation(err)) => {
            assert!(err.message.contains("required_field"));
        }
        _ => panic!("Expected validation error"),
    }
}

// Test nested struct with defaults
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
struct ServerConfig {
    host: String,
    port: u16,
    timeout: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "fallback-host".to_string(),
            port: 8000,
            timeout: 30,
        }
    }
}

#[derive(Debug, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
struct AppConfig {
    #[config(default = "default_server_config")]
    server: ServerConfig,

    app_name: String,
}

#[test]
fn test_nested_default_missing() {
    let toml = r#"
        app_name = "my-app"
    "#;

    let partial: PartialAppConfig = toml::from_str(toml).unwrap();
    let result = AppConfig::try_from_partial(partial);

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.app_name, "my-app");
    assert_eq!(config.server.host, "default-host");
    assert_eq!(config.server.port, 9090);
    assert_eq!(config.server.timeout, 60);
}

#[test]
fn test_nested_default_provided() {
    let toml = r#"
        app_name = "my-app"

        [server]
        host = "prod-host"
        port = 443
        timeout = 120
    "#;

    let partial: PartialAppConfig = toml::from_str(toml).unwrap();
    let result = AppConfig::try_from_partial(partial);

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.app_name, "my-app");
    assert_eq!(config.server.host, "prod-host");
    assert_eq!(config.server.port, 443);
    assert_eq!(config.server.timeout, 120);
}

#[test]
fn test_nested_partial_fields() {
    // Test that when server is partially provided, the missing fields
    // in the nested struct use their own fallbacks (not the default_server_config)
    let toml = r#"
        app_name = "my-app"

        [server]
        host = "partial-host"
    "#;

    let partial: PartialAppConfig = toml::from_str(toml).unwrap();
    let result = AppConfig::try_from_partial(partial);

    // Should fail because server.port and server.timeout are required
    assert!(result.is_err());
}

// Test optional fields with defaults (edge case - should work same as required)
#[derive(Debug, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
struct OptionalConfig {
    // Optional field without default attribute
    optional_no_default: Option<String>,

    // Note: Using #[config(default)] on Option<T> doesn't make much sense
    // since Option already defaults to None, but we should handle it
    #[config(default = "default_port")]
    optional_with_default: Option<u16>,
}

#[test]
fn test_optional_fields() {
    let toml = r#"
    "#;

    let partial: PartialOptionalConfig = toml::from_str(toml).unwrap();
    let result = OptionalConfig::try_from_partial(partial);

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.optional_no_default, None);
    // The default function is only called for non-Option types
    // For Option types, the field is already optional so defaults don't apply
    assert_eq!(config.optional_with_default, None);
}

// Test mixing defaulted and non-defaulted fields
#[derive(Debug, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
struct MixedConfig {
    #[config(default = "default_port")]
    port: u16,

    host: String, // Required, no default

    #[config(default = "default_timeout")]
    timeout: u64,

    optional: Option<String>, // Optional, no default needed
}

#[test]
fn test_mixed_config_success() {
    let toml = r#"
        host = "example.com"
    "#;

    let partial: PartialMixedConfig = toml::from_str(toml).unwrap();
    let result = MixedConfig::try_from_partial(partial);

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.port, 8080); // Default
    assert_eq!(config.host, "example.com"); // Provided
    assert_eq!(config.timeout, 30); // Default
    assert_eq!(config.optional, None); // Optional, not provided
}

#[test]
fn test_mixed_config_missing_required() {
    let toml = r#"
        port = 9000
    "#;

    let partial: PartialMixedConfig = toml::from_str(toml).unwrap();
    let result = MixedConfig::try_from_partial(partial);

    // Should error because 'host' is required and has no default
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    match &errors[0] {
        Diagnostic::Error(hearthd_config::Error::Validation(err)) => {
            assert!(err.message.contains("host"));
        }
        _ => panic!("Expected validation error"),
    }
}
