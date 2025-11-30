use std::collections::HashMap;

use hearthd_config::MergeableConfig;
use hearthd_config::TryFromPartial;
use hearthd_config::Validate;
use serde::Deserialize;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    #[default]
    Info,
    Debug,
}

#[derive(Debug, Default, TryFromPartial, MergeableConfig)]
pub struct SimpleConfig {
    pub level: LogLevel,
    pub overrides: HashMap<String, LogLevel>,
}

impl Validate for SimpleConfig {}

#[test]
fn test_basic_derive() {
    let config = SimpleConfig::default();
    assert_eq!(config.level, LogLevel::Info);
}

#[test]
fn test_basic_merge() {
    use std::fs;

    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");
    let config2_path = temp_dir.path().join("config2.toml");

    fs::write(
        &config1_path,
        r#"
        level = "debug"
        "#,
    )
    .unwrap();

    fs::write(
        &config2_path,
        r#"
        [overrides]
        foo = "info"
        "#,
    )
    .unwrap();

    let configs = PartialSimpleConfig::load_with_imports(&[config1_path, config2_path]).unwrap();
    let (merged, diagnostics) = PartialSimpleConfig::merge(configs);

    assert_eq!(diagnostics.len(), 0);
    assert_eq!(merged.level.unwrap().into_inner(), LogLevel::Debug);
    assert_eq!(
        merged
            .overrides
            .unwrap()
            .get("foo")
            .unwrap()
            .clone()
            .into_inner(),
        LogLevel::Info
    );

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_conflict_detection() {
    use std::fs;

    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config1_path = temp_dir.path().join("config1.toml");
    let config2_path = temp_dir.path().join("config2.toml");

    fs::write(
        &config1_path,
        r#"
        level = "info"
        "#,
    )
    .unwrap();

    fs::write(
        &config2_path,
        r#"
        level = "debug"
        "#,
    )
    .unwrap();

    let configs = PartialSimpleConfig::load_with_imports(&[config1_path, config2_path]).unwrap();
    let (_, diagnostics) = PartialSimpleConfig::merge(configs);

    assert_eq!(diagnostics.len(), 1);
    match &diagnostics[0] {
        hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(err)) => {
            assert!(err.field_path.contains("level"));
        }
        _ => panic!("Expected merge error"),
    }

    fs::remove_dir_all(&temp_dir).ok();
}
