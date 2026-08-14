use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use serde::Deserialize;

fn default_locations() -> Vec<String> {
    Vec::new()
}

/// Configuration for the met.no weather integration.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct Config {
    /// Names of `[locations]` entries to publish weather for.
    ///
    /// Empty disables the integration: no network requests, no nodes.
    #[config(default = "default_locations")]
    pub locations: Vec<String>,
}
