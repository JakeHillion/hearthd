use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use serde::Deserialize;

fn default_discovery_prefix() -> String {
    "homeassistant".to_string()
}

/// Configuration for the MQTT integration
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct Config {
    /// MQTT broker hostname or IP address
    pub broker: String,

    /// MQTT broker port
    pub port: u16,

    /// MQTT client ID
    pub client_id: String,

    /// Discovery prefix for Zigbee2MQTT (default: "homeassistant")
    #[config(default = "default_discovery_prefix")]
    pub discovery_prefix: String,

    /// Optional username for authentication
    pub username: Option<String>,

    /// Optional password for authentication
    pub password: Option<String>,
}
