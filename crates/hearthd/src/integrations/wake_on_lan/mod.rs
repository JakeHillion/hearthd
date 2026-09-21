mod config;
// Private module - allowed by clippy.toml allow-private-module-inception
#[allow(clippy::module_inception)]
mod wake_on_lan;

pub use config::Config as WolConfig;
use linkme::distributed_slice;
pub use wake_on_lan::WolIntegration;

use crate::engine;

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init_wol(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let wol_config = if let Some(c) = &ctx.config.integrations.wake_on_lan {
        c
    } else {
        return Ok(None);
    };

    // Nothing to monitor: the integration has no reason to run.
    if wol_config.hosts.is_empty() {
        return Ok(None);
    }

    Ok(Some(Box::new(WolIntegration::new(wol_config.clone()))))
}
