/// Configuration for the MQTT integration
#[derive(Debug, Clone)]
pub struct MqttConfig {
    /// MQTT broker hostname or IP address
    pub broker: String,

    /// MQTT broker port
    pub port: u16,

    /// Discovery prefix for Zigbee2MQTT (default: "homeassistant")
    pub discovery_prefix: String,

    /// MQTT client ID
    pub client_id: String,

    /// Optional username for authentication
    pub username: Option<String>,

    /// Optional password for authentication
    pub password: Option<String>,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            broker: "localhost".to_string(),
            port: 1883,
            discovery_prefix: "homeassistant".to_string(),
            client_id: "hearthd".to_string(),
            username: None,
            password: None,
        }
    }
}
