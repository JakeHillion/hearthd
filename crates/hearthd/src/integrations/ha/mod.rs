//! Home Assistant integration adapter.
//!
//! This module bridges the HA sandbox system with the Engine's integration trait system.

use std::error::Error;
use std::path::PathBuf;

use async_trait::async_trait;
use hearthd_config::{SubConfig, TryFromPartial};
use linkme::distributed_slice;
use serde::Deserialize;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::engine;
use crate::ha;

/// Configuration for Home Assistant integration.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct HaConfig {
    /// Enable the HA integration (default: true when section is present)
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for HaConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Home Assistant integration that runs integrations in sandboxed Python.
pub struct HaIntegration {
    name: String,
    registry_handle: Option<JoinHandle<()>>,
}

impl HaIntegration {
    pub fn new(name: String) -> Self {
        Self {
            name,
            registry_handle: None,
        }
    }
}

#[async_trait]
impl engine::Integration for HaIntegration {
    fn name(&self) -> &str {
        &self.name
    }

    async fn setup(
        &mut self,
        _tx: engine::FromIntegrationSender,
    ) -> Result<(), Box<dyn Error + Send>> {
        info!("[{}] Setting up Home Assistant integration", self.name);

        // Determine paths
        // For now, use relative paths from the working directory
        let python_path = PathBuf::from("python3");

        // Use vendor/ha-core as the HA source path
        let ha_source_path = PathBuf::from("vendor/ha-core");

        if !ha_source_path.exists() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Home Assistant source not found at: {}. \
                    Did you initialize the git submodule? Run: git submodule update --init",
                    ha_source_path.display()
                ),
            )));
        }

        // Create sandbox builder
        let builder = ha::SandboxBuilder::new(
            "met_oslo".to_string(), // Integration instance name
            python_path,
            ha_source_path,
        );

        // Create registry and register the sandbox
        let mut registry = ha::Registry::default();
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
    Ok(Some(Box::new(HaIntegration::new("ha".to_string()))))
}
