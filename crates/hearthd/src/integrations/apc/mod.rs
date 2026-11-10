mod config;
mod profile;
mod snmp;
// Private module - allowed by clippy.toml allow-private-module-inception
#[allow(clippy::module_inception)]
mod apc;

pub use apc::ApcIntegration;
pub use config::Config as ApcConfig;
use linkme::distributed_slice;

use crate::engine;

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init_apc(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let config = match &ctx.config.integrations.apc {
        Some(c) => c,
        None => return Ok(None),
    };

    if config.pdus.is_empty() {
        return Ok(None);
    }

    Ok(Some(Box::new(ApcIntegration::new(config)?)))
}
