use hearthd_config::FromConfig;

/// Configuration for the MQTT integration
#[derive(Debug, Clone, FromConfig)]
pub struct Config {
    /// MQTT broker hostname or IP address
    pub broker: String,

    /// MQTT broker port
    pub port: u16,

    /// MQTT client ID
    pub client_id: String,

    /// Discovery prefix for Zigbee2MQTT (default: "homeassistant")
    #[config(default = "homeassistant")]
    pub discovery_prefix: String,

    /// Optional username for authentication
    pub username: Option<String>,

    /// Optional password for authentication
    pub password: Option<String>,
}
