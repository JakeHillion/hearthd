use super::Integration;
use super::Result;
use super::SandboxBuilder;

use std::collections::BTreeMap;

/// Registry for storing and managing the lifetime of running HA sandboxes.
#[derive(Debug, Default)]
pub struct Registry {
    integrations: BTreeMap<String, Integration>,
}

impl Registry {
    pub async fn register(&mut self, builder: SandboxBuilder) -> super::Result<()> {
        let sb = builder.try_into_sandbox().await?;
        self.integrations.insert(builder.name, Integration::new(sb));
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
