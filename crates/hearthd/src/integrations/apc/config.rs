use std::collections::HashMap;

use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use serde::Deserialize;

fn default_port() -> u16 {
    161
}

fn default_read_community() -> String {
    "public".to_string()
}

fn default_poll_interval_ms() -> u64 {
    10_000
}

fn default_timeout_ms() -> u64 {
    2_000
}

fn default_true() -> bool {
    true
}

/// Configuration for the APC integration.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct Config {
    /// PDUs to monitor and switch, keyed by the name their nodes are keyed
    /// under.
    pub pdus: HashMap<String, PduConfig>,
}

/// One PDU.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
pub struct PduConfig {
    /// Which model this is, e.g. "apc_ap7921". The device must report the
    /// same model before the integration will talk to it further.
    pub model: String,

    /// IP address or hostname of the PDU's management card.
    pub host: String,

    /// SNMP agent UDP port (default: 161).
    #[config(default = "default_port")]
    pub port: u16,

    /// Community for reads (default: "public").
    #[config(default = "default_read_community")]
    pub read_community: String,

    /// Community for switching outlets.
    pub write_community: String,

    /// How often outlet state and load are read, in milliseconds.
    #[config(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,

    /// How long one SNMP request may take before it is retried, in
    /// milliseconds.
    #[config(default = "default_timeout_ms")]
    pub timeout_ms: u64,

    /// Per-outlet settings, keyed by outlet number (from 1). Outlets not
    /// listed are enabled and switchable.
    pub outlets: HashMap<String, OutletConfig>,
}

/// Settings for one outlet.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
pub struct OutletConfig {
    /// Whether the outlet gets a node at all (default: true).
    #[config(default = "default_true")]
    pub enabled: bool,

    /// Whether commands may switch the outlet (default: true). When false the
    /// node still reports state, and no write is ever sent for the outlet.
    #[config(default = "default_true")]
    pub switchable: bool,
}
