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
use super::integration::Integration;
use super::integration::IntegrationSender;
use super::integration::Source;
use super::integration::Stamped;
use super::integration::StreamReceiver;
use super::integration::StreamSender;
use super::integration::ToIntegrationSender;
use super::message::ToIntegrationMessage;
use super::node_id::NodeKey;
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

    /// What each node id is bound to: the integration that announced it and
    /// its local key. Routes invokes and writes, and is what makes a node id
    /// mean one thing for as long as it is bound.
    bindings: std::sync::Mutex<HashMap<NodeId, NodeKey>>,

    /// Communication channels to integrations (for commands)
    integration_channels: HashMap<String, ToIntegrationSender>,

    /// The consuming end of the event stream, held by `run`.
    event_rx: Mutex<StreamReceiver>,

    /// The producing end of the event stream, cloned to every producer.
    event_tx: StreamSender,

    /// Handles for integration tasks
    integration_handles: Vec<JoinHandle<()>>,

    /// Source of node ids for every integration, so that no two can name the
    /// same node.
    node_ids: NodeIdAllocator,
}

/// Capacity of the event stream. Provides backpressure when producers send
/// faster than the engine can process.
const EVENT_CHANNEL_SIZE: usize = 1024;

fn engine_error(message: String) -> Box<dyn Error + Send> {
    Box::new(std::io::Error::other(message))
}

impl Engine {
    /// Create a new Engine instance
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
        Self {
            state: ArcSwap::new(Arc::default()),
            bindings: std::sync::Mutex::new(HashMap::new()),
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

    /// The stream handle an integration of this name produces through.
    fn sender_for(&self, name: &str) -> IntegrationSender {
        IntegrationSender::new(name, self.event_tx.clone(), self.node_ids.clone())
    }

    /// Register an integration with the engine
    ///
    /// This spawns the integration in a background task, wires up channels,
    /// and starts its setup process.
    pub fn register_integration(&mut self, name: String, mut integration: Box<dyn Integration>) {
        let (to_integration_tx, mut to_integration_rx) = mpsc::unbounded_channel();
        let sender = self.sender_for(&name);

        self.integration_channels
            .insert(name.clone(), to_integration_tx);

        // Spawn integration task
        let handle = tokio::spawn(async move {
            // Setup integration (gives it the sender for events)
            if let Err(e) = integration.setup(sender).await {
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

    /// What `node_id` is currently bound to, if anything.
    fn binding(&self, node_id: NodeId) -> Result<Option<NodeKey>, Box<dyn Error + Send>> {
        let bindings = self
            .bindings
            .lock()
            .map_err(|e| engine_error(e.to_string()))?;
        Ok(bindings.get(&node_id).cloned())
    }

    /// The binding for `node_id`, which must be owned by `source`.
    fn owned_binding(
        &self,
        node_id: NodeId,
        source: &Source,
    ) -> Result<NodeKey, Box<dyn Error + Send>> {
        let key = self.binding(node_id)?.ok_or_else(|| {
            engine_error(format!("node {node_id} is not bound to any integration"))
        })?;
        match source {
            Source::Integration(name) if *name == key.integration => Ok(key),
            _ => Err(engine_error(format!(
                "node {node_id} belongs to {}, not to {source}",
                key.integration
            ))),
        }
    }

    /// Route a message to the integration that owns the target node.
    fn send_to_owner(
        &self,
        node_id: NodeId,
        message: impl FnOnce(NodeKey) -> ToIntegrationMessage,
    ) -> Result<(), Box<dyn Error + Send>> {
        let key = self
            .binding(node_id)?
            .ok_or_else(|| engine_error(format!("No integration found for node: {node_id}")))?;

        let tx = self
            .integration_channels
            .get(key.integration.as_ref())
            .ok_or_else(|| {
                engine_error(format!(
                    "Integration channel not found: {}",
                    key.integration
                ))
            })?;

        tx.send(message(key))
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })
    }

    /// Run the engine's main event loop
    ///
    /// Consumes the event stream in arrival order and updates state.
    pub async fn run(&self) -> Result<(), Box<dyn Error + Send>> {
        info!("Engine starting");

        let mut rx = self.event_rx.lock().await;
        while let Some(stamped) = rx.recv().await {
            if let Err(e) = self.handle_event(stamped).await {
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

    /// Put an event on the stream on behalf of the API.
    ///
    /// The one way anything other than an integration produces onto the
    /// stream. Waits while the queue is full, and fails only once the engine
    /// has stopped consuming.
    pub async fn submit(&self, event: Event) -> Result<(), Box<dyn Error + Send>> {
        self.event_tx
            .send(Stamped {
                source: Source::Api,
                event,
            })
            .await
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })
    }

    /// Handle one event off the stream.
    async fn handle_event(&self, stamped: Stamped) -> Result<(), Box<dyn Error + Send>> {
        let Stamped { source, event } = stamped;
        match event {
            Event::NodeAdded { node_id, node } => {
                let Source::Integration(integration) = &source else {
                    return Err(engine_error(format!(
                        "node {node_id} ({}) announced by {source}, which owns no nodes",
                        node.entity_id
                    )));
                };
                let key = NodeKey {
                    integration: integration.clone(),
                    local: node.key.clone(),
                };

                {
                    let mut bindings = self
                        .bindings
                        .lock()
                        .map_err(|e| engine_error(e.to_string()))?;
                    if let Some(existing) = bindings.get(&node_id) {
                        if *existing != key {
                            return Err(engine_error(format!(
                                "node {node_id} is bound to {existing}, refusing to rebind it to {key}"
                            )));
                        }
                    }
                    bindings.insert(node_id, key.clone());
                }

                info!("Node added: {} ({}) from {}", node_id, node.entity_id, key);

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
                self.owned_binding(node_id, &source)?;
                info!("Node removed: {}", node_id);

                {
                    let mut state = State::clone(&self.state.load());
                    if let Some(node) = state.nodes.remove(&node_id) {
                        state.by_entity_id.remove(&node.entity_id);
                    }
                    self.state.store(Arc::new(state));
                }

                if let Ok(mut bindings) = self.bindings.lock() {
                    bindings.remove(&node_id);
                }
            }
            Event::Report {
                node_id,
                endpoint_id,
                cluster,
            } => {
                self.owned_binding(node_id, &source)?;
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
                self.send_to_owner(node_id, |key| ToIntegrationMessage::InvokeCommand {
                    node_id,
                    key: key.local,
                    endpoint_id,
                    command,
                })?;
            }
            Event::Write {
                node_id,
                endpoint_id,
                write,
            } => {
                info!(
                    "Write: node={} endpoint={} write={:?}",
                    node_id, endpoint_id, write
                );
                self.send_to_owner(node_id, |key| ToIntegrationMessage::WriteAttribute {
                    node_id,
                    key: key.local,
                    endpoint_id,
                    write,
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
    use crate::matter::AttributeWrite;
    use crate::matter::Cluster;
    use crate::matter::ClusterCommand;
    use crate::matter::LocalKey;
    use crate::matter::Node;
    use crate::matter::OnOffCluster;
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

        async fn setup(&mut self, _tx: IntegrationSender) -> Result<(), Box<dyn Error + Send>> {
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

    fn lamp(key: &str) -> Node {
        Node {
            key: LocalKey::from(key),
            entity_id: "light.lamp".into(),
            name: None,
            endpoints: HashMap::new(),
        }
    }

    type Running = JoinHandle<Result<(), Box<dyn Error + Send>>>;

    /// A running engine with the recorder registered, and the channel the
    /// recorder forwards to.
    fn running_engine() -> (
        Arc<Engine>,
        mpsc::UnboundedReceiver<ToIntegrationMessage>,
        Running,
    ) {
        let mut engine = Engine::new();
        let (tx, rx) = mpsc::unbounded_channel();
        engine.register_integration("recorder".into(), Box::new(Recorder { tx }));
        let engine = Arc::new(engine);
        let running = tokio::spawn({
            let engine = engine.clone();
            async move { engine.run().await }
        });
        (engine, rx, running)
    }

    /// A running engine with one node announced by the recorder.
    async fn engine_with_recorded_node() -> (
        Arc<Engine>,
        NodeId,
        mpsc::UnboundedReceiver<ToIntegrationMessage>,
        Running,
    ) {
        let (engine, rx, running) = running_engine();
        let recorder = engine.sender_for("recorder");
        let node_id = recorder.allocator().allocate();
        recorder
            .send(Event::NodeAdded {
                node_id,
                node: lamp("lamp-1"),
            })
            .await
            .expect("engine is running");
        (engine, node_id, rx, running)
    }

    /// Wait for the engine to have drained everything queued so far.
    async fn settled(engine: &Engine) {
        // The API path goes through the same queue, so once a probe report
        // for an unbound node has been dequeued everything before it has too.
        for _ in 0..50 {
            if engine.event_tx.capacity() == EVENT_CHANNEL_SIZE {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("engine did not drain its queue");
    }

    #[tokio::test]
    async fn an_invoke_on_the_stream_reaches_the_owning_integration_by_key() {
        let (engine, node_id, mut rx, running) = engine_with_recorded_node().await;
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
                key,
                endpoint_id: 1,
                command: ClusterCommand::OnOff(OnOffCommand::On),
            } if n == node_id && key == LocalKey::from("lamp-1")
        ));
        running.abort();
    }

    #[tokio::test]
    async fn a_write_on_the_stream_reaches_the_owning_integration_by_key() {
        let (engine, node_id, mut rx, running) = engine_with_recorded_node().await;
        let write = AttributeWrite {
            cluster: "Thermostat".into(),
            attribute: "system_mode".into(),
            value: serde_json::json!("Cool"),
        };
        engine
            .submit(Event::Write {
                node_id,
                endpoint_id: 1,
                write: write.clone(),
            })
            .await
            .expect("engine is running");

        let msg = rx.recv().await.expect("the integration receives the write");
        assert!(matches!(
            msg,
            ToIntegrationMessage::WriteAttribute {
                node_id: n,
                key,
                endpoint_id: 1,
                write: w,
            } if n == node_id && key == LocalKey::from("lamp-1") && w == write
        ));
        running.abort();
    }

    #[tokio::test]
    async fn a_node_announced_by_the_api_is_refused() {
        let (engine, _rx, running) = running_engine();
        let node_id = engine.node_ids.allocate();
        engine
            .submit(Event::NodeAdded {
                node_id,
                node: lamp("lamp-1"),
            })
            .await
            .expect("engine is running");
        settled(&engine).await;

        assert!(engine.state_snapshot().nodes.is_empty());
        assert_eq!(engine.binding(node_id).unwrap(), None);
        running.abort();
    }

    #[tokio::test]
    async fn a_node_id_cannot_be_rebound_to_a_different_key() {
        let (engine, node_id, _rx, running) = engine_with_recorded_node().await;
        engine
            .sender_for("recorder")
            .send(Event::NodeAdded {
                node_id,
                node: lamp("lamp-2"),
            })
            .await
            .expect("engine is running");
        settled(&engine).await;

        let binding = engine.binding(node_id).unwrap().expect("still bound");
        assert_eq!(binding.local, LocalKey::from("lamp-1"));
        assert_eq!(
            engine.state_snapshot().nodes[&node_id].key,
            LocalKey::from("lamp-1")
        );
        running.abort();
    }

    #[tokio::test]
    async fn a_report_from_an_integration_that_does_not_own_the_node_is_dropped() {
        let (engine, node_id, _rx, running) = engine_with_recorded_node().await;
        let report = Event::Report {
            node_id,
            endpoint_id: 1,
            cluster: Cluster::OnOff(OnOffCluster { on_off: true }),
        };

        engine
            .sender_for("impostor")
            .send(report.clone())
            .await
            .expect("engine is running");
        settled(&engine).await;
        assert!(engine.state_snapshot().nodes[&node_id].endpoints.is_empty());

        engine
            .sender_for("recorder")
            .send(report)
            .await
            .expect("engine is running");
        settled(&engine).await;
        assert!(
            engine.state_snapshot().nodes[&node_id]
                .endpoints
                .contains_key(&1)
        );
        running.abort();
    }

    #[tokio::test]
    async fn removal_releases_the_binding() {
        let (engine, node_id, _rx, running) = engine_with_recorded_node().await;
        engine
            .sender_for("recorder")
            .send(Event::NodeRemoved { node_id })
            .await
            .expect("engine is running");
        settled(&engine).await;

        assert!(engine.state_snapshot().nodes.is_empty());
        assert_eq!(engine.binding(node_id).unwrap(), None);
        running.abort();
    }
}
