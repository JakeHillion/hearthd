use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use serde::Deserialize;

/// Configuration for the Snapcast integration.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct Config {
    /// Snapserver hostname or IP address.
    pub host: String,

    /// Snapserver JSON-RPC TCP port (default: 1705).
    #[config(default = 1705_u16)]
    pub port: u16,

    /// Reconnect interval in milliseconds (default: 5000).
    #[config(default = 5000_u64)]
    pub reconnect_interval_ms: u64,
}
