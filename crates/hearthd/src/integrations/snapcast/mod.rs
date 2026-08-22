mod client;
mod config;
mod mapper;
#[cfg(test)]
mod mapper_tests;
mod models;
// Private module - allowed by clippy.toml allow-private-module-inception
#[allow(clippy::module_inception)]
mod snapcast;

pub use config::Config as SnapcastConfig;
use linkme::distributed_slice;
pub use snapcast::SnapcastIntegration;

use crate::engine;

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init_snapcast(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let snapcast_config = match &ctx.config.integrations.snapcast {
        Some(c) => c,
        None => return Ok(None),
    };

    Ok(Some(Box::new(SnapcastIntegration::new(
        snapcast_config.clone(),
    ))))
}
