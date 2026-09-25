use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::error;
use tracing::info;
use tracing::warn;

use super::event::Event;
use super::integration::EventReceiver;
use super::integration::EventSender;
use super::integration::Integration;
use super::integration::ToIntegrationSender;
use super::message::ToIntegrationMessage;
use super::state::State;
use crate::engine::IntegrationContext;
use crate::engine::NodeId;
use crate::engine::NodeIdAllocator;

/// hearthd engine
///
/// This structure handles the flow of events, applying automations to them, sending them to the
/// correct integration, and maintaining a view of the world with State.
pub struct Engine {
    /// Centralized state snapshot (readers load the Arc, writer stores a new one)
    state: ArcSwap<State>,

    /// Map of NodeId -> integration name for routing commands.
    node_integration_map: std::sync::Mutex<HashMap<NodeId, String>>,

    /// Communication channels to integrations (for commands)
    integration_channels: HashMap<String, ToIntegrationSender>,

    /// The consuming end of the event stream, held by `run`.
    event_rx: Mutex<EventReceiver>,

    /// The producing end of the event stream, cloned to every producer.
    event_tx: EventSender,

    /// Handles for integration tasks
    integration_handles: Vec<JoinHandle<()>>,

    /// Source of node ids for every integration, so that no two can name the
    /// same node.
    node_ids: NodeIdAllocator,
}

/// Capacity of the event stream. Provides backpressure when producers send
/// faster than the engine can process.
const EVENT_CHANNEL_SIZE: usize = 1024;

impl Engine {
    /// Create a new Engine instance
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
        Self {
            state: ArcSwap::new(Arc::default()),
            node_integration_map: std::sync::Mutex::new(HashMap::new()),
            integration_channels: HashMap::new(),
            event_rx: Mutex::new(event_rx),
            event_tx,
            integration_handles: Vec::new(),
            node_ids: NodeIdAllocator::new(),
        }
    }

    /// Register integrations from configuration
    ///
    /// This is a convenience method that checks the config and registers
    /// any enabled integrations.
    pub fn register_integrations_from_config(
        &mut self,
        cfg: &crate::config::Config,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ctx = IntegrationContext { config: cfg };
        for constr in super::integration::REGISTRY {
            let integration = match constr(&ctx) {
                Ok(Some(i)) => i,
                Err(e) => {
                    error!("failed to setup integration: {}", e);
                    continue;
                }
                Ok(None) => continue,
            };
            let name = integration.name().to_string();
            self.register_integration(name, integration);
        }

        Ok(())
    }

    /// Register an integration with the engine
    ///
    /// This spawns the integration in a background task, wires up channels,
    /// and starts its setup process.
    pub fn register_integration(&mut self, name: String, mut integration: Box<dyn Integration>) {
        let (to_integration_tx, mut to_integration_rx) = mpsc::unbounded_channel();
        let event_tx = self.event_tx.clone();
        let node_ids = self.node_ids.clone();

        self.integration_channels
            .insert(name.clone(), to_integration_tx);

        // Spawn integration task
        let handle = tokio::spawn(async move {
            // Setup integration (gives it the sender for events)
            if let Err(e) = integration.setup(event_tx, node_ids).await {
                warn!("Integration '{}' setup failed: {}", name, e);
                return;
            }

            // Process commands from engine
            while let Some(msg) = to_integration_rx.recv().await {
                if let Err(e) = integration.handle_message(msg).await {
                    warn!("Integration '{}' failed to handle message: {}", name, e);
                }
            }

            if let Err(e) = integration.shutdown().await {
                warn!("Integration '{}' shutdown failed: {}", name, e);
            }
        });

        self.integration_handles.push(handle);
    }

    /// Route a command to the integration that owns the target node.
    fn send_command(&self, msg: ToIntegrationMessage) -> Result<(), Box<dyn Error + Send>> {
        let node_id = match &msg {
            ToIntegrationMessage::InvokeCommand { node_id, .. } => *node_id,
        };

        let map = self
            .node_integration_map
            .lock()
            .map_err(|e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::other(e.to_string()))
            })?;

        let integration_name = map.get(&node_id).ok_or_else(|| -> Box<dyn Error + Send> {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("No integration found for node: {}", node_id),
            ))
        })?;

        let tx = self.integration_channels.get(integration_name).ok_or_else(
            || -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Integration channel not found: {}", integration_name),
                ))
            },
        )?;

        tx.send(msg)
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })
    }

    /// Run the engine's main event loop
    ///
    /// Consumes the event stream in arrival order and updates state.
    pub async fn run(&self) -> Result<(), Box<dyn Error + Send>> {
        info!("Engine starting");

        let mut rx = self.event_rx.lock().await;
        while let Some(event) = rx.recv().await {
            if let Err(e) = self.handle_event(event).await {
                warn!("Error handling event: {}", e);
            }
        }

        info!("Engine shutting down");
        Ok(())
    }

    /// Get a snapshot of the current engine state.
    ///
    /// Clones the `Arc` (atomic refcount bump), essentially free.
    pub fn state_snapshot(&self) -> Arc<State> {
        self.state.load_full()
    }

    /// Resolve an entity_id alias to a NodeId via the state's reverse index.
    pub fn resolve_entity_id(&self, entity_id: &str) -> Option<NodeId> {
        self.state.load().by_entity_id.get(entity_id).copied()
    }

    /// Put an event on the stream.
    ///
    /// The one way anything other than an integration produces onto the
    /// stream. Waits while the queue is full, and fails only once the engine
    /// has stopped consuming.
    pub async fn submit(&self, event: Event) -> Result<(), Box<dyn Error + Send>> {
        self.event_tx
            .send(event)
            .await
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })
    }

    /// Handle one event off the stream.
    async fn handle_event(&self, event: Event) -> Result<(), Box<dyn Error + Send>> {
        match event {
            Event::NodeAdded { node_id, node } => {
                info!(
                    "Node added: {} ({}) from {}",
                    node_id, node.entity_id, node.integration
                );

                if let Ok(mut map) = self.node_integration_map.lock() {
                    map.insert(node_id, node.integration.clone());
                }

                for (endpoint_id, endpoint) in &node.endpoints {
                    for (device_type, cluster_id) in endpoint.missing_mandatory_clusters() {
                        warn!(
                            "Node {} endpoint {} declares {} without its mandatory cluster {:#06x}",
                            node_id,
                            endpoint_id,
                            device_type.name(),
                            cluster_id
                        );
                    }
                }

                {
                    let mut state = State::clone(&self.state.load());
                    // Re-announcing an existing node is how an integration
                    // reports a rename, so drop the name it used to answer to
                    // rather than leaving a second alias that outlives the
                    // node and survives its removal.
                    if let Some(previous) = state.nodes.get(&node_id) {
                        if previous.entity_id != node.entity_id {
                            let previous_entity_id = previous.entity_id.clone();
                            state.by_entity_id.remove(&previous_entity_id);
                        }
                    }
                    state.by_entity_id.insert(node.entity_id.clone(), node_id);
                    state.nodes.insert(node_id, node);
                    self.state.store(Arc::new(state));
                }
            }
            Event::NodeRemoved { node_id } => {
                info!("Node removed: {}", node_id);

                {
                    let mut state = State::clone(&self.state.load());
                    if let Some(node) = state.nodes.remove(&node_id) {
                        state.by_entity_id.remove(&node.entity_id);
                    }
                    self.state.store(Arc::new(state));
                }

                if let Ok(mut map) = self.node_integration_map.lock() {
                    map.remove(&node_id);
                }
            }
            Event::Report {
                node_id,
                endpoint_id,
                cluster,
            } => {
                info!(
                    "Report: node={} endpoint={} cluster={}",
                    node_id,
                    endpoint_id,
                    cluster.name()
                );

                {
                    let mut state = State::clone(&self.state.load());
                    if let Some(node) = state.nodes.get_mut(&node_id) {
                        let endpoint = node.endpoints.entry(endpoint_id).or_default();
                        endpoint
                            .clusters
                            .insert(cluster.name().to_string(), cluster);
                    }
                    self.state.store(Arc::new(state));
                }
            }
            Event::Invoke {
                node_id,
                endpoint_id,
                command,
            } => {
                info!(
                    "Invoke: node={} endpoint={} command={:?}",
                    node_id, endpoint_id, command
                );
                self.send_command(ToIntegrationMessage::InvokeCommand {
                    node_id,
                    endpoint_id,
                    command,
                })?;
            }
        }
        Ok(())
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::matter::ClusterCommand;
    use crate::matter::Node;
    use crate::matter::OnOffCommand;

    /// Forwards every message the engine sends it to a channel the test reads.
    struct Recorder {
        tx: mpsc::UnboundedSender<ToIntegrationMessage>,
    }

    #[async_trait]
    impl Integration for Recorder {
        fn name(&self) -> &str {
            "recorder"
        }

        async fn setup(
            &mut self,
            _tx: EventSender,
            _node_ids: NodeIdAllocator,
        ) -> Result<(), Box<dyn Error + Send>> {
            Ok(())
        }

        async fn handle_message(
            &mut self,
            msg: ToIntegrationMessage,
        ) -> Result<(), Box<dyn Error + Send>> {
            self.tx
                .send(msg)
                .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })
        }

        async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn an_invoke_on_the_stream_reaches_the_owning_integration() {
        let mut engine = Engine::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        engine.register_integration("recorder".into(), Box::new(Recorder { tx }));
        let engine = Arc::new(engine);
        let running = tokio::spawn({
            let engine = engine.clone();
            async move { engine.run().await }
        });

        let node_id = engine.node_ids.allocate();
        engine
            .submit(Event::NodeAdded {
                node_id,
                node: Node {
                    entity_id: "light.lamp".into(),
                    integration: "recorder".into(),
                    name: None,
                    endpoints: HashMap::new(),
                },
            })
            .await
            .expect("engine is running");
        engine
            .submit(Event::Invoke {
                node_id,
                endpoint_id: 1,
                command: ClusterCommand::OnOff(OnOffCommand::On),
            })
            .await
            .expect("engine is running");

        let msg = rx
            .recv()
            .await
            .expect("the integration receives the command");
        assert!(matches!(
            msg,
            ToIntegrationMessage::InvokeCommand {
                node_id: n,
                endpoint_id: 1,
                command: ClusterCommand::OnOff(OnOffCommand::On),
            } if n == node_id
        ));
        running.abort();
    }
}
