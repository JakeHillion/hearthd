use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use linkme::distributed_slice;
use tokio::sync::mpsc;

use super::event::Event;
use super::message::ToIntegrationMessage;
use super::node_id::NodeId;
use super::node_id::NodeIdAllocator;
use crate::config::Config;
use crate::matter::Cluster;
use crate::matter::EndpointId;
use crate::matter::LocalKey;
use crate::matter::Node;

/// Who put an event on the stream.
///
/// Stamped by the sender rather than carried in the event, so an integration
/// cannot claim to be another one and the engine knows the owner of every
/// node from the announcement alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Integration(Arc<str>),
    Api,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Integration(name) => f.write_str(name),
            Source::Api => f.write_str("api"),
        }
    }
}

/// One item on the engine's stream: an event and who produced it.
#[derive(Debug, Clone)]
pub struct Stamped {
    pub source: Source,
    pub event: Event,
}

/// The engine's event stream. Bounded, so a producer that outruns the engine
/// waits rather than growing the queue without limit.
pub type StreamSender = mpsc::Sender<Stamped>;
pub type StreamReceiver = mpsc::Receiver<Stamped>;

/// The engine has stopped consuming its stream, so there is nobody left to
/// publish to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("engine stream closed")]
pub struct StreamClosed;

/// Channel types for messages FROM the engine TO integrations (unbounded - engine must not block)
pub type ToIntegrationSender = mpsc::UnboundedSender<ToIntegrationMessage>;

/// Result type for integration factory functions
pub type IntegrationFactoryResult = anyhow::Result<Option<Box<dyn Integration>>>;

/// An integration's handle onto the stream. Every event through it is
/// stamped with the integration's name.
#[derive(Debug, Clone)]
pub struct IntegrationSender {
    integration: Arc<str>,
    tx: StreamSender,
    node_ids: NodeIdAllocator,
}

impl IntegrationSender {
    pub fn new(
        integration: impl Into<Arc<str>>,
        tx: StreamSender,
        node_ids: NodeIdAllocator,
    ) -> Self {
        Self {
            integration: integration.into(),
            tx,
            node_ids,
        }
    }

    /// Put an event on the stream under this integration's name. Waits while
    /// the queue is full, and fails only once the engine has stopped
    /// consuming.
    pub async fn send(&self, event: Event) -> Result<(), StreamClosed> {
        self.tx
            .send(Stamped {
                source: Source::Integration(self.integration.clone()),
                event,
            })
            .await
            .map_err(|_| StreamClosed)
    }

    /// The engine's node id allocator. Ids must come from here: the keyspace
    /// is shared with every other integration, and one picked locally will
    /// eventually collide with one of theirs.
    pub fn allocator(&self) -> NodeIdAllocator {
        self.node_ids.clone()
    }

    fn node_id(&self, key: &LocalKey) -> NodeId {
        NodeId::derive(&self.integration, key)
    }

    /// Announce a node, or re-announce one whose shape or name changed. Its
    /// id is derived from this integration's name and the node's key.
    pub async fn node_added(&self, node: Node) -> Result<(), StreamClosed> {
        let node_id = self.node_id(&node.key);
        self.send(Event::NodeAdded { node_id, node }).await
    }

    /// Report a cluster snapshot for the node this integration calls `key`.
    pub async fn report(
        &self,
        key: &LocalKey,
        endpoint_id: EndpointId,
        cluster: Cluster,
    ) -> Result<(), StreamClosed> {
        self.send(Event::Report {
            node_id: self.node_id(key),
            endpoint_id,
            cluster,
        })
        .await
    }

    /// Withdraw the node this integration calls `key`.
    pub async fn node_removed(&self, key: &LocalKey) -> Result<(), StreamClosed> {
        self.send(Event::NodeRemoved {
            node_id: self.node_id(key),
        })
        .await
    }
}

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
    /// The integration receives its handle onto the engine's event stream, to
    /// put its reports and node lifecycle on.
    async fn setup(&mut self, tx: IntegrationSender) -> Result<(), Box<dyn Error + Send>>;

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
