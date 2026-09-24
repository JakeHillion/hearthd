//! EcoFlow integration, over EcoFlow's consumer ("private") cloud API.
//!
//! Scoped to the **EcoFlow Wave 3** portable air conditioner: climate control,
//! the on-board battery and power telemetry, condensate handling and the
//! panel options.
//!
//! Out of scope, deliberately:
//!
//! - The public developer API (access-key/secret-key signing, `api-e.ecoflow.com`,
//!   `/iot-open/...`). It is a different authentication scheme with a
//!   JSON-only message format, and it does not expose the Wave 3.
//! - Every other EcoFlow device. Each family has its own protobuf schema and
//!   its own field numbering; nothing transfers between them except the
//!   transport.
//! - Local control over Bluetooth or the device's own access point. The Wave 3
//!   is cloud-only here.
//!
//! # Layout
//!
//! | Module | Contents |
//! | --- | --- |
//! | `config` | declared account and devices |
//! | `protobuf` | a minimal protobuf reader and writer |
//! | `cloud` | authentication, topics, MQTT transport, reconnect pacing |
//! | `wave3` | the device: framing, field tables, semantics, state, Matter mapping |
//! | `ecoflow` | session lifecycle and command dispatch |
//!
//! # Attribution
//!
//! EcoFlow does not document any of this. The protocol was reverse-engineered
//! by the [`tolwi/hassio-ecoflow-cloud`] Home Assistant custom component, read
//! at commit `a7ebbba`, which is licensed Apache-2.0 — the same licence as
//! hearthd, so derivation is permitted.
//!
//! What hearthd took from that project is *knowledge of the wire format*: the
//! authentication handshake, the MQTT client-id construction, the frame header
//! constants, the payload obfuscation scheme, the message field numbers, and
//! the control semantics. Every one of those facts is recorded in a module
//! that names the upstream file it came from, so the boundary is legible in
//! the source rather than only in a licence header. Those modules are
//! `cloud::auth`, `cloud::topics`, `wave3::wire`, `wave3::fields` and
//! `wave3::semantics`.
//!
//! Everything else is hearthd's own: the protobuf codec, the frame
//! encoder/decoder, the delta-merge state model, the mapping onto hearthd's
//! Matter primitives, the async session handling, and all of the tests. No
//! code was copied from that project.
//!
//! [`tolwi/hassio-ecoflow-cloud`]: https://github.com/tolwi/hassio-ecoflow-cloud

pub mod cloud;
pub mod config;
pub mod protobuf;
pub mod wave3;

// Private module - allowed by clippy.toml allow-private-module-inception
#[allow(clippy::module_inception)]
mod ecoflow;

use std::collections::HashMap;

use anyhow::Context;
pub use config::Config as EcoFlowConfig;
pub use ecoflow::EcoFlowIntegration;
use linkme::distributed_slice;

use crate::engine;

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init_ecoflow(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let config = match &ctx.config.integrations.ecoflow {
        Some(config) => config,
        None => return Ok(None),
    };

    if config.devices.is_empty() {
        // Devices cannot be discovered, so an EcoFlow section with none
        // declared can do nothing at all. Say so rather than starting a
        // session that will never produce a node.
        anyhow::bail!("EcoFlow is configured but declares no devices");
    }

    // Two config keys naming one serial are two nodes fed by one device: the
    // entity ids differ, so registration cannot catch it, and the telemetry
    // reverse index would silently keep only one of them.
    let mut by_serial: HashMap<&str, &str> = HashMap::with_capacity(config.devices.len());
    for (name, device) in &config.devices {
        if let Some(existing) = by_serial.insert(&device.serial, name) {
            anyhow::bail!(
                "EcoFlow devices '{existing}' and '{name}' both declare serial '{}'",
                device.serial
            );
        }
    }

    let api = cloud::auth::HttpApi::new(config.api_host.clone())
        .context("failed to create the EcoFlow API client")?;
    let transport = cloud::transport::RumqttcTransport::new();

    Ok(Some(Box::new(EcoFlowIntegration::new(
        api, transport, config,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::integrations::ecoflow::config::DeviceConfig;

    fn config_with(devices: Vec<(&str, &str)>) -> Config {
        let mut config = Config::default();
        config.integrations.ecoflow = Some(EcoFlowConfig {
            api_host: "example.invalid".to_string(),
            email: "someone@example.invalid".to_string(),
            password: "hunter2".to_string(),
            devices: devices
                .into_iter()
                .map(|(name, serial)| {
                    (
                        name.to_string(),
                        DeviceConfig {
                            serial: serial.to_string(),
                            name: None,
                        },
                    )
                })
                .collect(),
        });
        config
    }

    /// Two keys for one serial give two nodes with distinct entity ids, so
    /// registration cannot catch it: both would register, and the serial
    /// reverse index would then feed telemetry to only one of them.
    #[test]
    fn two_devices_declaring_one_serial_are_rejected() {
        let config = config_with(vec![("bedroom", "AB123"), ("study", "AB123")]);
        let err = match init_ecoflow(&engine::IntegrationContext { config: &config }) {
            Err(err) => err,
            Ok(_) => panic!("one device cannot back two nodes"),
        };
        assert!(
            err.to_string().contains("both declare serial"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn distinct_serials_are_accepted() {
        let config = config_with(vec![("bedroom", "AB123"), ("study", "CD456")]);
        let integration = init_ecoflow(&engine::IntegrationContext { config: &config })
            .unwrap_or_else(|e| panic!("distinct serials are fine: {e}"));
        assert!(integration.is_some());
    }
}
