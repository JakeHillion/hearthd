// Test that SubConfig types can be used across modules without requiring
// PartialFoo to be directly in scope. This validates the HasPartialConfig trait approach.

use hearthd_config::MergeableConfig;

// Define a SubConfig in a separate module to simulate cross-module usage
mod external_config {
    use hearthd_config::SubConfig;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Deserialize, SubConfig)]
    pub struct DatabaseConfig {
        pub host: String,
        pub port: u16,
        pub name: String,
    }
}

// Import only the main type, NOT PartialDatabaseConfig
// This simulates the real-world scenario where PartialDatabaseConfig is not in scope
use external_config::DatabaseConfig;

#[derive(Debug, MergeableConfig)]
pub struct AppConfig {
    pub app_name: String,
    pub database: DatabaseConfig,
}

#[test]
fn test_cross_module_config() {
    use std::fs;

    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    fs::write(
        &config_path,
        r#"
        app_name = "MyApp"

        [database]
        host = "localhost"
        port = 5432
        name = "mydb"
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config_path]).unwrap();
    let (merged, diagnostics) = PartialAppConfig::merge(configs);

    assert_eq!(diagnostics.len(), 0, "Expected no diagnostics");
    assert_eq!(merged.app_name.unwrap().into_inner(), "MyApp".to_string());

    let db = merged.database.unwrap();
    assert_eq!(db.host.unwrap().into_inner(), "localhost".to_string());
    assert_eq!(db.port.unwrap().into_inner(), 5432);
    assert_eq!(db.name.unwrap().into_inner(), "mydb".to_string());

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_cross_module_merge() {
    use std::fs;

    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");
    let config2_path = temp_dir.path().join("config2.toml");

    fs::write(
        &config1_path,
        r#"
        app_name = "MyApp"

        [database]
        host = "localhost"
        port = 5432
        "#,
    )
    .unwrap();

    fs::write(
        &config2_path,
        r#"
        [database]
        name = "mydb"
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config1_path, config2_path]).unwrap();
    let (merged, diagnostics) = PartialAppConfig::merge(configs);

    assert_eq!(diagnostics.len(), 0, "Expected no diagnostics");

    let db = merged.database.unwrap();
    assert_eq!(db.host.unwrap().into_inner(), "localhost".to_string());
    assert_eq!(db.port.unwrap().into_inner(), 5432);
    assert_eq!(db.name.unwrap().into_inner(), "mydb".to_string());

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_cross_module_conflict_detection() {
    use std::fs;

    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");
    let config2_path = temp_dir.path().join("config2.toml");

    fs::write(
        &config1_path,
        r#"
        app_name = "MyApp"

        [database]
        host = "localhost"
        port = 5432
        name = "mydb"
        "#,
    )
    .unwrap();

    fs::write(
        &config2_path,
        r#"
        [database]
        port = 3306
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config1_path, config2_path]).unwrap();
    let (_, diagnostics) = PartialAppConfig::merge(configs);

    // Should detect a conflict on database.port
    assert_eq!(diagnostics.len(), 1);
    match &diagnostics[0] {
        hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(err)) => {
            assert!(
                err.field_path.contains("database.port"),
                "Expected conflict on database.port, got: {}",
                err.field_path
            );
        }
        _ => panic!("Expected merge error, got: {:?}", diagnostics[0]),
    }

    fs::remove_dir_all(&temp_dir).ok();
}
