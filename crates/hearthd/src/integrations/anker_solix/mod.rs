//! Local SOLIX S2000 (AS220) control through the system BlueZ daemon.
//! Each configured unit owns one BLE connection; enrollment needs a Power
//! button press, while normal startup reuses its existing client UUID.

#[cfg(not(target_os = "linux"))]
compile_error!(
    "feature `integration_anker_solix` requires Linux with system BlueZ; disable it when building for other platforms"
);

pub mod cli;
mod config;
mod mapper;
mod protocol;
mod runtime;
mod transport;
mod types;

pub use config::Config;
use linkme::distributed_slice;

use crate::engine;

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let Some(config) = &ctx.config.integrations.anker_solix else {
        return Ok(None);
    };
    if config.devices.is_empty() {
        return Ok(None);
    }
    Ok(Some(Box::new(runtime::SolixIntegration::new(
        config.clone(),
    ))))
}
