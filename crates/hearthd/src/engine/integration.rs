use std::error::Error;

use async_trait::async_trait;
use linkme::distributed_slice;
use tokio::sync::mpsc;

use super::event::Event;
use super::message::ToIntegrationMessage;
use super::node_id::NodeIdAllocator;
use crate::config::Config;

/// The engine's event stream. Bounded, so a producer that outruns the engine
/// waits rather than growing the queue without limit.
pub type EventSender = mpsc::Sender<Event>;
pub type EventReceiver = mpsc::Receiver<Event>;

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
    /// The integration receives the engine's event stream to put its reports
    /// and node lifecycle on, and an allocator for the node ids it
    /// declares. Ids must come from that allocator: the keyspace is shared
    /// with every other integration, and one it picks itself will eventually
    /// collide with one of theirs.
    async fn setup(
        &mut self,
        tx: EventSender,
        node_ids: NodeIdAllocator,
    ) -> Result<(), Box<dyn Error + Send>>;

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
