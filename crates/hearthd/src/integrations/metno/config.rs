use hearthd_config::FromConfig;

/// Configuration for the met.no weather integration.
#[derive(Debug, Clone, FromConfig)]
pub struct Config {
    /// Names of `[locations]` entries to publish weather for.
    ///
    /// Empty disables the integration: no network requests, no nodes.
    pub locations: Vec<String>,
}
