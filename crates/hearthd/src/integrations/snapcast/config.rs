use hearthd_config::FromConfig;

/// Configuration for the Snapcast integration.
#[derive(Debug, Clone, FromConfig)]
pub struct Config {
    /// Snapserver hostname or IP address.
    pub host: String,

    /// Snapserver JSON-RPC TCP port (default: 1705).
    #[config(default = 1705)]
    pub port: u16,

    /// Reconnect interval in milliseconds (default: 5000).
    #[config(default = 5000)]
    pub reconnect_interval_ms: u64,
}
