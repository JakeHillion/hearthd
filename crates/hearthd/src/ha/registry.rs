use super::integration::IntegrationConfig;
use super::Integration;
use super::Result;
use super::SandboxBuilder;

use std::collections::BTreeMap;

use crate::engine;

/// Registry for storing and managing the lifetime of running HA sandboxes.
#[derive(Debug)]
pub struct Registry {
    integrations: BTreeMap<String, Integration>,
    engine_tx: engine::FromIntegrationSender,
}

impl Registry {
    pub fn new(engine_tx: engine::FromIntegrationSender) -> Self {
        Self {
            integrations: BTreeMap::new(),
            engine_tx,
        }
    }

    pub async fn register(&mut self, builder: SandboxBuilder) -> super::Result<()> {
        let sb = builder.try_into_sandbox().await?;
        let config = IntegrationConfig::default();
        let name = builder.name;
        self.integrations.insert(
            name,
            Integration::with_config_and_tx(sb, config, self.engine_tx.clone()),
        );
        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        if self.integrations.len() > 1 {
            todo!("Registry::run with >1 integrations");
        }
        if self.integrations.is_empty() {
            return Ok(());
        }

        if let Some(i) = self.integrations.values_mut().next() {
            i.run().await
        } else {
            Ok(())
        }
    }
}
