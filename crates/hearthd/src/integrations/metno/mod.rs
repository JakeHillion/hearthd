mod config;
mod forecast;
// Private module - allowed by clippy.toml allow-private-module-inception
#[allow(clippy::module_inception)]
mod metno;

use anyhow::Context;
pub use config::Config as MetnoConfig;
use linkme::distributed_slice;
pub use metno::MetnoIntegration;

use crate::engine;

/// A resolved weather site: a configured location name paired with the
/// coordinates looked up from `[locations]`.
#[derive(Debug, Clone)]
pub struct Site {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_m: Option<f64>,
}

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init_metno(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let metno_config = match &ctx.config.integrations.metno {
        Some(c) if !c.locations.is_empty() => c,
        _ => return Ok(None),
    };

    // An unknown name is a hard error rather than a silently ignored typo.
    let mut sites = Vec::with_capacity(metno_config.locations.len());
    for name in &metno_config.locations {
        let loc =
            ctx.config.locations.locations.get(name).with_context(|| {
                format!("metno: location '{name}' is not defined in [locations]")
            })?;
        sites.push(Site {
            name: name.clone(),
            latitude: loc.latitude,
            longitude: loc.longitude,
            elevation_m: loc.elevation_m,
        });
    }

    Ok(Some(Box::new(MetnoIntegration::new(sites))))
}
