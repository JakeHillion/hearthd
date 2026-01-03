use std::error::Error;

use async_trait::async_trait;
use linkme::distributed_slice;
use tokio::sync::mpsc;

use super::message::FromIntegrationMessage;
use super::message::ToIntegrationMessage;
use crate::config::Config;

/// Channel types for messages FROM integrations TO the engine
/// These are bounded channels (capacity 256) to provide backpressure
pub type FromIntegrationSender = mpsc::Sender<FromIntegrationMessage>;
pub type FromIntegrationReceiver = mpsc::Receiver<FromIntegrationMessage>;

/// Channel types for messages FROM the engine TO integrations (unbounded - engine must not block)
pub type ToIntegrationSender = mpsc::UnboundedSender<ToIntegrationMessage>;

/// Result type for integration factory functions
pub type IntegrationFactoryResult = anyhow::Result<Option<Box<dyn Integration>>>;

pub struct IntegrationContext<'a> {
    pub config: &'a Config,
}

#[distributed_slice]
pub static REGISTRY: [fn(&IntegrationContext) -> IntegrationFactoryResult];

/// Integration trait that all integrations must implement
#[async_trait]
pub trait Integration: Send + Sync {
    /// Get the name/identifier of this integration
    fn name(&self) -> &str;

    /// Set up the integration - subscribe to topics, initialize state, etc.
    ///
    /// The integration receives a sender to report events back to the engine
    /// (discovery, state changes, etc.)
    async fn setup(&mut self, tx: FromIntegrationSender) -> Result<(), Box<dyn Error + Send>>;

    /// Handle a command from the engine
    ///
    /// The integration should execute the requested action (e.g., turn on a light)
    async fn handle_message(
        &mut self,
        msg: ToIntegrationMessage,
    ) -> Result<(), Box<dyn Error + Send>>;

    /// Shut down the integration gracefully
    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>>;
}
