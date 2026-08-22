use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use serde::Deserialize;

fn default_port() -> u16 {
    1705
}

fn default_reconnect_interval_ms() -> u64 {
    5000
}

/// Configuration for the Snapcast integration.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct Config {
    /// Snapserver hostname or IP address.
    pub host: String,

    /// Snapserver JSON-RPC TCP port (default: 1705).
    #[config(default = "default_port")]
    pub port: u16,

    /// Reconnect interval in milliseconds (default: 5000).
    #[config(default = "default_reconnect_interval_ms")]
    pub reconnect_interval_ms: u64,
}
