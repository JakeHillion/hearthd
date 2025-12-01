use std::error::Error;

use async_trait::async_trait;
use linkme::distributed_slice;
use tokio::sync::mpsc;

use super::message::Message;
use crate::config::Config;

/// Channel types for integration communication
pub type MessageSender = mpsc::UnboundedSender<Message>;
pub type MessageReceiver = mpsc::UnboundedReceiver<Message>;

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
    /// The integration receives a MessageSender to send FromIntegration messages
    /// back to the engine (discovery, state changes, etc.)
    async fn setup(&mut self, tx: MessageSender) -> Result<(), Box<dyn Error + Send>>;

    /// Handle a message from the engine (ToIntegration direction)
    ///
    /// The integration should execute the requested action (e.g., turn on a light)
    async fn handle_message(&mut self, msg: Message) -> Result<(), Box<dyn Error + Send>>;

    /// Shut down the integration gracefully
    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>>;
}
