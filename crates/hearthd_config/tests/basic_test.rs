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

// Note: from_files is not auto-generated when manual validation is needed.
// The hearthd crate tests the full functionality with its Config struct.
