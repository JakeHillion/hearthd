mod config;
pub mod cli;
mod mapper;
mod protocol;
mod runtime;
mod transport;

pub use config::Config;

use linkme::distributed_slice;

use crate::engine;

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let Some(config) = &ctx.config.integrations.anker_solix else { return Ok(None) };
    config.validate()?;
    if config.devices.is_empty() { return Ok(None) }
    Ok(Some(Box::new(runtime::SolixIntegration::new(config.clone()))))
}
