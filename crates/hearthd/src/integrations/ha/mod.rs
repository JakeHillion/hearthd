//! Home Assistant integration adapter.
//!
//! This module bridges the HA sandbox system with the Engine's integration trait system.

use std::error::Error;
use std::path::Path;

use async_trait::async_trait;
use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use linkme::distributed_slice;
use serde::Deserialize;
use tokio::task::JoinHandle;
use tracing::info;
use tracing::warn;

use crate::engine;
use crate::ha;

/// Configuration for Home Assistant integration.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct HaConfig {
    /// Enable the HA integration (default: true when section is present)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Python interpreter to run integrations with. It must carry Home
    /// Assistant's dependencies; hearthd does not install them.
    ///
    /// Defaults to the interpreter chosen when hearthd was built, which is how
    /// a packaged build finds one. There is no fallback to `python3` on `PATH`:
    /// see [`crate::ha::paths`].
    pub python_interpreter: Option<String>,

    /// Directory holding hearthd's `runner.py` and `homeassistant-shim/`.
    ///
    /// Defaults to the copy chosen at build time, or to this crate's source
    /// tree for an unpackaged build.
    pub python_assets: Option<String>,

    /// Directory holding the `homeassistant` package whose `components` supply
    /// the integrations to run.
    pub ha_source: Option<String>,

    /// `PYTHONPATH` for the child process, carrying the components'
    /// dependencies. Unnecessary when the interpreter already bundles them.
    pub python_path: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for HaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            python_interpreter: None,
            python_assets: None,
            ha_source: None,
            python_path: None,
        }
    }
}

/// Home Assistant integration that runs integrations in sandboxed Python.
pub struct HaIntegration {
    name: String,
    config: HaConfig,
    registry_handle: Option<JoinHandle<()>>,
}

impl HaIntegration {
    pub fn new(name: String, config: HaConfig) -> Self {
        Self {
            name,
            config,
            registry_handle: None,
        }
    }

    /// Where this integration's Python assets live, per configuration and the
    /// locations baked in when hearthd was built.
    fn paths(&self) -> Result<ha::Paths, ha::paths::Error> {
        ha::Paths::resolve(ha::paths::Overrides {
            interpreter: self.config.python_interpreter.as_ref().map(Path::new),
            assets: self.config.python_assets.as_ref().map(Path::new),
            ha_source: self.config.ha_source.as_ref().map(Path::new),
            python_path: self.config.python_path.as_deref(),
        })
    }
}

#[async_trait]
impl engine::Integration for HaIntegration {
    fn name(&self) -> &str {
        &self.name
    }

    async fn setup(
        &mut self,
        tx: engine::FromIntegrationSender,
        node_ids: engine::NodeIdAllocator,
    ) -> Result<(), Box<dyn Error + Send>> {
        info!("[{}] Setting up Home Assistant integration", self.name);

        let paths = self
            .paths()
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })?;

        info!(
            "[{}] Using Python interpreter: {}",
            self.name,
            paths.interpreter.display()
        );
        info!(
            "[{}] Running Home Assistant components from {}",
            self.name,
            paths.ha_source.display()
        );

        // Create sandbox builder
        let builder = ha::SandboxBuilder::new(
            "met_oslo".to_string(), // Integration instance name
            paths,
        );

        // Create registry with engine sender and register the sandbox
        let mut registry = ha::Registry::new(tx, node_ids);
        registry
            .register(builder)
            .await
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })?;

        // Spawn the registry to run in the background
        let name = self.name.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = registry.run().await {
                warn!("[{}] HA registry error: {}", name, e);
            }
        });

        self.registry_handle = Some(handle);

        info!("[{}] Home Assistant integration started", self.name);
        Ok(())
    }

    async fn handle_message(
        &mut self,
        msg: engine::ToIntegrationMessage,
    ) -> Result<(), Box<dyn Error + Send>> {
        // For now, log messages but don't route them
        // TODO: Route commands to the appropriate sandbox
        info!("[{}] Received message: {:?}", self.name, msg);
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
        info!("[{}] Shutting down Home Assistant integration", self.name);

        if let Some(handle) = self.registry_handle.take() {
            handle.abort();
            match handle.await {
                Ok(()) => info!("[{}] HA registry stopped", self.name),
                Err(e) if e.is_cancelled() => {
                    info!("[{}] HA registry task cancelled", self.name)
                }
                Err(e) => warn!("[{}] HA registry task error: {}", self.name, e),
            }
        }

        Ok(())
    }
}

#[distributed_slice(engine::INTEGRATION_REGISTRY)]
fn init_ha(ctx: &engine::IntegrationContext) -> engine::IntegrationFactoryResult {
    let ha_config = if let Some(c) = &ctx.config.integrations.ha {
        c
    } else {
        return Ok(None);
    };

    if !ha_config.enabled {
        return Ok(None);
    }

    info!("Initializing Home Assistant integration");
    Ok(Some(Box::new(HaIntegration::new(
        "ha".to_string(),
        ha_config.clone(),
    ))))
}
