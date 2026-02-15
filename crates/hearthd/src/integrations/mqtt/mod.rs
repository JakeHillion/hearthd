mod binary_sensor;
mod client;
mod config;
mod discovery;
mod light;
// Private module - allowed by clippy.toml allow-private-module-inception
#[allow(clippy::module_inception)]
mod mqtt;

use anyhow::Context;
pub use config::Config as MqttConfig;
use linkme::distributed_slice;
pub use mqtt::MqttIntegration;

use crate::engine;

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init_mqtt(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let mqtt_config = if let Some(c) = &ctx.config.integrations.mqtt {
        c
    } else {
        return Ok(None);
    };

    let client = client::RumqttcClient::new(mqtt_config).context("Failed to create MQTT client")?;
    Ok(Some(Box::new(MqttIntegration::new(client, mqtt_config))))
}
