mod client;
mod config;
mod mapper;
#[cfg(test)]
mod mapper_tests;
mod models;
#[allow(clippy::module_inception)]
mod snapcast;

pub use config::Config as SnapcastConfig;
use linkme::distributed_slice;
pub use snapcast::SnapcastIntegration;

use crate::engine;

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init_snapcast(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let snapcast_config = if let Some(c) = &ctx.config.integrations.snapcast {
        c
    } else {
        return Ok(None);
    };

    let node_ids = engine::NodeIdAllocator::for_test();
    Ok(Some(Box::new(SnapcastIntegration::new(
        snapcast_config.clone(),
        node_ids,
    ))))
}
