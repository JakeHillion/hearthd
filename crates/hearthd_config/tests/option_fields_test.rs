use std::fs;

use hearthd_config::MergeableConfig;
use hearthd_config::SubConfig;
use serde::Deserialize;
use tempfile::TempDir;

// Test structs for Option<ComplexStruct> testing

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, SubConfig)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, SubConfig)]
pub struct CacheConfig {
    pub ttl_seconds: u32,
    pub max_size: u64,
}

#[derive(Debug, MergeableConfig)]
pub struct AppConfig {
    pub name: String,
    pub database: Option<DatabaseConfig>,
    pub cache: Option<CacheConfig>,
    pub description: Option<String>, // Simple option type for comparison
}

// Test 1: Basic Option<SimpleType> fields
#[test]
fn test_option_simple_type() {
    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");

    fs::write(
        &config1_path,
        r#"
        name = "MyApp"
        description = "First description"
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config1_path]).unwrap();
    let (merged, diagnostics) = PartialAppConfig::merge(configs);

    assert_eq!(diagnostics.len(), 0, "Should merge without conflicts");
    assert_eq!(merged.name.unwrap().into_inner(), "MyApp");
    assert_eq!(
        merged.description.unwrap().into_inner(),
        "First description"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

// Test 2: Option<ComplexStruct> basic test - core bug fix verification
#[test]
fn test_option_complex_struct_basic() {
    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");

    fs::write(
        &config1_path,
        r#"
        name = "MyApp"

        [database]
        host = "localhost"
        port = 5432
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config1_path]).unwrap();
    let (merged, diagnostics) = PartialAppConfig::merge(configs);

    assert_eq!(diagnostics.len(), 0, "Should merge without conflicts");
    assert!(merged.database.is_some(), "Database should be present");

    let db = merged.database.unwrap();
    assert_eq!(db.host.unwrap().into_inner(), "localhost");
    assert_eq!(db.port.unwrap().into_inner(), 5432);

    fs::remove_dir_all(&temp_dir).ok();
}

// Test 3: Option<ComplexStruct> field-level merge
#[test]
fn test_option_complex_struct_field_merge() {
    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");
    let config2_path = temp_dir.path().join("config2.toml");

    // First file provides host
    fs::write(
        &config1_path,
        r#"
        name = "MyApp"

        [database]
        host = "localhost"
        "#,
    )
    .unwrap();

    // Second file provides port
    fs::write(
        &config2_path,
        r#"
        [database]
        port = 5432
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config1_path, config2_path]).unwrap();
    let (merged, diagnostics) = PartialAppConfig::merge(configs);

    assert_eq!(
        diagnostics.len(),
        0,
        "Should merge fields from both files without conflicts"
    );
    assert!(merged.database.is_some(), "Database should be present");

    let db = merged.database.unwrap();
    assert_eq!(
        db.host.unwrap().into_inner(),
        "localhost",
        "Host from first file"
    );
    assert_eq!(db.port.unwrap().into_inner(), 5432, "Port from second file");

    fs::remove_dir_all(&temp_dir).ok();
}

// Test 4: Option<ComplexStruct> conflict detection
#[test]
fn test_option_complex_struct_conflict() {
    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");
    let config2_path = temp_dir.path().join("config2.toml");

    fs::write(
        &config1_path,
        r#"
        name = "MyApp"

        [database]
        host = "localhost"
        port = 5432
        "#,
    )
    .unwrap();

    // Second file has conflicting port
    fs::write(
        &config2_path,
        r#"
        [database]
        port = 3306
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config1_path, config2_path]).unwrap();
    let (merged, diagnostics) = PartialAppConfig::merge(configs);

    assert!(
        !diagnostics.is_empty(),
        "Should detect conflict in nested field"
    );
    match &diagnostics[0] {
        hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(err)) => {
            assert!(
                err.field_path.contains("database.port"),
                "Conflict should be on database.port field"
            );
        }
        _ => panic!("Expected merge error"),
    }

    // Even with conflict, non-conflicting fields should merge
    assert!(merged.database.is_some());
    let db = merged.database.unwrap();
    assert_eq!(db.host.unwrap().into_inner(), "localhost");

    fs::remove_dir_all(&temp_dir).ok();
}

// Test 5: Multiple Option<ComplexStruct> fields
#[test]
fn test_multiple_option_complex_structs() {
    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");
    let config2_path = temp_dir.path().join("config2.toml");

    fs::write(
        &config1_path,
        r#"
        name = "MyApp"

        [database]
        host = "localhost"
        port = 5432
        "#,
    )
    .unwrap();

    fs::write(
        &config2_path,
        r#"
        [cache]
        ttl_seconds = 300
        max_size = 1024
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config1_path, config2_path]).unwrap();
    let (merged, diagnostics) = PartialAppConfig::merge(configs);

    assert_eq!(diagnostics.len(), 0, "Should merge without conflicts");
    assert!(
        merged.database.is_some(),
        "Database should be present from first file"
    );
    assert!(
        merged.cache.is_some(),
        "Cache should be present from second file"
    );

    let db = merged.database.unwrap();
    assert_eq!(db.host.unwrap().into_inner(), "localhost");
    assert_eq!(db.port.unwrap().into_inner(), 5432);

    let cache = merged.cache.unwrap();
    assert_eq!(cache.ttl_seconds.unwrap().into_inner(), 300);
    assert_eq!(cache.max_size.unwrap().into_inner(), 1024);

    fs::remove_dir_all(&temp_dir).ok();
}

// Test 6: Mixed simple and complex option types
#[test]
fn test_mixed_option_types() {
    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");
    let config2_path = temp_dir.path().join("config2.toml");

    fs::write(
        &config1_path,
        r#"
        name = "MyApp"
        description = "A test application"

        [database]
        host = "localhost"
        "#,
    )
    .unwrap();

    fs::write(
        &config2_path,
        r#"
        [database]
        port = 5432
        username = "admin"
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config1_path, config2_path]).unwrap();
    let (merged, diagnostics) = PartialAppConfig::merge(configs);

    assert_eq!(diagnostics.len(), 0, "Should merge without conflicts");

    // Simple option type
    assert_eq!(
        merged.description.unwrap().into_inner(),
        "A test application"
    );

    // Complex option type with field merging
    assert!(merged.database.is_some());
    let db = merged.database.unwrap();
    assert_eq!(db.host.unwrap().into_inner(), "localhost");
    assert_eq!(db.port.unwrap().into_inner(), 5432);
    // Nested option within complex option
    assert_eq!(db.username.unwrap().into_inner(), "admin");

    fs::remove_dir_all(&temp_dir).ok();
}

// Test 7: None in first file, Some in second file
#[test]
fn test_option_complex_none_then_some() {
    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");
    let config2_path = temp_dir.path().join("config2.toml");

    // First file has no database
    fs::write(
        &config1_path,
        r#"
        name = "MyApp"
        "#,
    )
    .unwrap();

    // Second file provides database
    fs::write(
        &config2_path,
        r#"
        [database]
        host = "localhost"
        port = 5432
        "#,
    )
    .unwrap();

    let configs = PartialAppConfig::load_with_imports(&[config1_path, config2_path]).unwrap();
    let (merged, diagnostics) = PartialAppConfig::merge(configs);

    assert_eq!(diagnostics.len(), 0, "Should merge without conflicts");
    assert!(
        merged.database.is_some(),
        "Database should be present from second file"
    );

    let db = merged.database.unwrap();
    assert_eq!(db.host.unwrap().into_inner(), "localhost");
    assert_eq!(db.port.unwrap().into_inner(), 5432);

    fs::remove_dir_all(&temp_dir).ok();
}
