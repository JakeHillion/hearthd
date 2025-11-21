use hearthd_config::MergeableConfig;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    #[default]
    Info,
    Debug,
}

#[derive(Debug, Default, MergeableConfig)]
pub struct SimpleConfig {
    pub level: LogLevel,
    pub overrides: HashMap<String, LogLevel>,
}

#[test]
fn test_basic_derive() {
    // Just test that it compiles
    let config = SimpleConfig::default();
    assert_eq!(config.level, LogLevel::Info);
}

#[test]
fn test_from_files() {
    use std::fs;
    use std::io::Write;

    let temp_dir = std::env::temp_dir().join("hearthd_config_test");
    fs::create_dir_all(&temp_dir).unwrap();

    let config_path = temp_dir.join("test.toml");
    let mut file = fs::File::create(&config_path).unwrap();
    write!(file, r#"
level = "debug"

[overrides]
"test" = "info"
"#).unwrap();

    let result = SimpleConfig::from_files(&[config_path]);
    assert!(result.is_ok(), "Failed to load config: {:?}", result.err());

    let (config, _diag) = result.unwrap();
    assert_eq!(config.level, LogLevel::Debug);
    assert_eq!(config.overrides.get("test"), Some(&LogLevel::Info));

    fs::remove_dir_all(&temp_dir).ok();
}
