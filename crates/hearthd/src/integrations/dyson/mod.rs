//! Dyson Pure Cool (TP07) integration.
//!
//! This integration controls Dyson fan/purifier devices locally over MQTT,
//! using credentials obtained once from the Dyson cloud API. The cloud login
//! is performed by `hearthd integration dyson login`, which prints a TOML
//! snippet the user pastes into their secrets config.
//!
//! Supported on the TP07 / "Pure Cool" / "Purifier Cool" (device type 438):
//!
//! - On/off, speed 1-10, auto mode, oscillation toggle, night mode,
//!   continuous monitoring, sleep timer, airflow direction (front/diffuse).
//! - Temperature, humidity, PM2.5, PM10, NO₂, VOC, and filter life readings.
//!
//! # Layout
//!
//! | Module | Contents |
//! | --- | --- |
//! | `config` | declared devices and account region |
//! | `login` | `hearthd integration dyson login` implementation |
//! | `cloud` | Dyson app API client (OTP login, device manifest) |
//! | `transport` | MQTT session using `rumqttc` |
//! | `state` | JSON payload parser and device state |
//! | `mapping` | state -> Matter clusters, command -> MQTT payload |
//! | `dyson` | `Integration` trait implementation |

pub mod cloud;
pub mod config;
pub mod login;

// Private module - allowed by clippy.toml allow-private-module-inception
#[allow(clippy::module_inception)]
mod dyson;
mod mapping;
mod state;
mod transport;

pub use config::Config as DysonConfig;
pub use dyson::DysonIntegration;
use linkme::distributed_slice;

use crate::engine;

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init_dyson(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let config = match &ctx.config.integrations.dyson {
        Some(config) => config,
        None => return Ok(None),
    };

    if config.devices.is_empty() {
        anyhow::bail!("Dyson is configured but declares no devices");
    }

    Ok(Some(Box::new(DysonIntegration::new(config.clone()))))
}
