use async_trait::async_trait;
use std::error::Error;
use tokio::sync::mpsc;

use super::message::Message;

/// Channel types for integration communication
pub type MessageSender = mpsc::UnboundedSender<Message>;
pub type MessageReceiver = mpsc::UnboundedReceiver<Message>;

/// Integration trait that all integrations must implement
#[async_trait]
pub trait Integration: Send + Sync {
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
