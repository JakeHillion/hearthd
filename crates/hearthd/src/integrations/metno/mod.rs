mod config;
mod forecast;
// Private module - allowed by clippy.toml allow-private-module-inception
#[allow(clippy::module_inception)]
mod metno;

use std::collections::HashSet;

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

    // `locations` is a list, so nothing stops the same name appearing twice —
    // including by accident, when several config files are merged. Two sites
    // with one name are two nodes competing for one entity id, which the
    // registry would refuse one at a time; reject it here instead, where the
    // duplicate itself can be named.
    let mut seen = HashSet::with_capacity(metno_config.locations.len());
    for name in &metno_config.locations {
        if !seen.insert(name) {
            anyhow::bail!("metno: location '{name}' is listed more than once");
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn context_with(locations: Vec<&str>) -> Config {
        let mut config = Config::default();
        config.locations.locations.insert(
            "home".to_string(),
            crate::config::Location {
                latitude: 59.9139,
                longitude: 10.7522,
                elevation_m: None,
                timezone: None,
            },
        );
        config.integrations.metno = Some(MetnoConfig {
            locations: locations.into_iter().map(str::to_string).collect(),
        });
        config
    }

    #[test]
    fn a_location_listed_twice_is_rejected() {
        let config = context_with(vec!["home", "home"]);
        let err = match init_metno(&engine::IntegrationContext { config: &config }) {
            Err(err) => err,
            Ok(_) => panic!("two sites cannot share one entity id"),
        };
        assert!(
            err.to_string().contains("listed more than once"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_location_listed_once_is_accepted() {
        let config = context_with(vec!["home"]);
        let integration = init_metno(&engine::IntegrationContext { config: &config })
            .unwrap_or_else(|e| panic!("a single location is fine: {e}"));
        assert!(integration.is_some());
    }
}
