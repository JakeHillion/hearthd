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
use super::names::ResolveError;
use super::names::slug;
use super::node_id::NodeKey;
use super::state::State;
use crate::config::AliasesConfig;
use crate::engine::IntegrationContext;
use crate::engine::NodeId;
use crate::matter::LocalKey;
use crate::matter::Node;

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

    /// What each configured alias names. The ids they resolve to live in the
    /// state snapshot; this keeps the target for diagnostics.
    aliases: HashMap<String, NodeKey>,

    /// Communication channels to integrations (for commands)
    integration_channels: HashMap<String, ToIntegrationSender>,

    /// The consuming end of the event stream, held by `run`.
    event_rx: Mutex<StreamReceiver>,

    /// The producing end of the event stream, cloned to every producer.
    event_tx: StreamSender,

    /// Handles for integration tasks
    integration_handles: Vec<JoinHandle<()>>,
}

/// Capacity of the event stream. Provides backpressure when producers send
/// faster than the engine can process.
const EVENT_CHANNEL_SIZE: usize = 1024;

fn engine_error(message: String) -> Box<dyn Error + Send> {
    Box::new(std::io::Error::other(message))
}

/// The slugged name `node` is addressed by, if it has one.
fn discovered_name(node: &Node) -> Option<String> {
    node.name
        .as_deref()
        .map(slug)
        .filter(|name| !name.is_empty())
}

/// Take `node_id` out from under `name`, dropping the entry once empty.
fn forget_name(state: &mut State, name: &str, node_id: NodeId) {
    if let Some(ids) = state.names.get_mut(name) {
        ids.retain(|id| *id != node_id);
        if ids.is_empty() {
            state.names.remove(name);
        }
    }
}

impl Engine {
    /// Create a new Engine instance
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
        Self {
            state: ArcSwap::new(Arc::default()),
            bindings: std::sync::Mutex::new(HashMap::new()),
            aliases: HashMap::new(),
            integration_channels: HashMap::new(),
            event_rx: Mutex::new(event_rx),
            event_tx,
            integration_handles: Vec::new(),
        }
    }

    /// Install the configured aliases.
    ///
    /// Each alias names an integration and that integration's key for a
    /// node, which is exactly what the node's id is derived from, so every
    /// alias resolves from here on whether or not the node ever appears.
    pub fn load_aliases(&mut self, aliases: &AliasesConfig) {
        let mut state = State::clone(&self.state.load());
        for (name, target) in &aliases.aliases {
            let key = NodeKey {
                integration: target.integration.as_str().into(),
                local: LocalKey::from(target.key.as_str()),
            };
            let node_id = NodeId::derive(&key.integration, &key.local);
            state.aliases.insert(name.clone(), node_id);
            self.aliases.insert(name.clone(), key);
        }
        self.state.store(Arc::new(state));
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

        for (alias, key) in &self.aliases {
            if !self
                .integration_channels
                .contains_key(key.integration.as_ref())
            {
                warn!(
                    "alias {alias} names {key}, but no integration called {} is running",
                    key.integration
                );
            }
        }

        Ok(())
    }

    /// The stream handle an integration of this name produces through.
    fn sender_for(&self, name: &str) -> IntegrationSender {
        IntegrationSender::new(name, self.event_tx.clone())
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

    /// Resolve a name to a node id.
    ///
    /// An alias wins. Otherwise a name exactly one node was discovered
    /// under, otherwise the id itself in hex. Whether the node is currently
    /// announced is a separate question, answered by the state snapshot.
    pub fn resolve(&self, name: &str) -> Result<NodeId, ResolveError> {
        let state = self.state.load();
        if let Some(id) = state.aliases.get(name) {
            return Ok(*id);
        }
        match state.names.get(name).map(Vec::as_slice) {
            Some([id]) => return Ok(*id),
            Some(ids) => {
                return Err(ResolveError::Ambiguous {
                    name: name.to_string(),
                    ids: ids.to_vec(),
                });
            }
            None => {}
        }
        name.parse()
            .map_err(|_| ResolveError::Unknown(name.to_string()))
    }

    /// What a configured alias names, if `name` is one.
    pub fn alias_target(&self, name: &str) -> Option<&NodeKey> {
        self.aliases.get(name)
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
                        node.key
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

                info!("Node added: {} ({})", node_id, key);

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
                    let name = discovered_name(&node);

                    // A re-announcement under a new name is how an
                    // integration reports an upstream rename: the old name
                    // stops resolving rather than lingering as a second one.
                    if let Some(previous) = state.nodes.get(&node_id) {
                        let old =
                            discovered_name(previous).filter(|old| Some(old) != name.as_ref());
                        if let Some(old) = old {
                            forget_name(&mut state, &old, node_id);
                        }
                    }

                    if let Some(name) = name {
                        let ids = state.names.entry(name.clone()).or_default();
                        if !ids.contains(&node_id) {
                            ids.push(node_id);
                            ids.sort_unstable();
                        }
                        if ids.len() > 1 {
                            warn!(
                                "{name} now names {} nodes and resolves to none of them; an alias picks one",
                                ids.len()
                            );
                        }
                        if let Some(alias_id) = state.aliases.get(&name) {
                            if *alias_id != node_id {
                                warn!(
                                    "node {node_id} is named {name}, which the alias {name} already gives to node {alias_id}"
                                );
                            }
                        }
                    }

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
                        if let Some(name) = discovered_name(&node) {
                            forget_name(&mut state, &name, node_id);
                        }
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
    use crate::config::AliasTarget;
    use crate::matter::AttributeWrite;
    use crate::matter::Cluster;
    use crate::matter::ClusterCommand;
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

    fn named(key: &str, name: &str) -> Node {
        Node {
            name: Some(name.to_string()),
            ..lamp(key)
        }
    }

    fn recorder_id(key: &str) -> NodeId {
        NodeId::derive("recorder", &LocalKey::from(key))
    }

    /// An engine with one alias installed, before anything is registered.
    fn engine_with_alias(alias: &str, integration: &str, key: &str) -> Engine {
        let mut engine = Engine::new();
        let mut aliases = HashMap::new();
        aliases.insert(
            alias.to_string(),
            AliasTarget {
                integration: integration.to_string(),
                key: key.to_string(),
            },
        );
        engine.load_aliases(&AliasesConfig { aliases });
        engine
    }

    type Running = JoinHandle<Result<(), Box<dyn Error + Send>>>;

    /// Register the recorder on `engine` and start it, returning the channel
    /// the recorder forwards to.
    fn start(
        mut engine: Engine,
    ) -> (
        Arc<Engine>,
        mpsc::UnboundedReceiver<ToIntegrationMessage>,
        Running,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        engine.register_integration("recorder".into(), Box::new(Recorder { tx }));
        let engine = Arc::new(engine);
        let running = tokio::spawn({
            let engine = engine.clone();
            async move { engine.run().await }
        });
        (engine, rx, running)
    }

    /// A running engine with the recorder registered, and the channel the
    /// recorder forwards to.
    fn running_engine() -> (
        Arc<Engine>,
        mpsc::UnboundedReceiver<ToIntegrationMessage>,
        Running,
    ) {
        start(Engine::new())
    }

    /// A running engine with one node announced by the recorder.
    async fn engine_with_recorded_node() -> (
        Arc<Engine>,
        NodeId,
        mpsc::UnboundedReceiver<ToIntegrationMessage>,
        Running,
    ) {
        let (engine, rx, running) = running_engine();
        let node = lamp("lamp-1");
        let node_id = NodeId::derive("recorder", &node.key);
        engine
            .sender_for("recorder")
            .node_added(node)
            .await
            .expect("engine is running");
        (engine, node_id, rx, running)
    }

    /// Announce `node` as the recorder and wait for the engine to apply it.
    async fn announce(engine: &Engine, node: Node) {
        engine
            .sender_for("recorder")
            .node_added(node)
            .await
            .expect("engine is running");
        settled(engine).await;
    }

    /// Put `event` on the stream as if `integration` had sent it, without
    /// the sender deriving the node id: how a test forges an announcement or
    /// report for an id the sender would never produce.
    async fn stamped_as(engine: &Engine, integration: &str, event: Event) {
        engine
            .event_tx
            .send(Stamped {
                source: Source::Integration(integration.into()),
                event,
            })
            .await
            .expect("engine is running");
    }

    /// Wait for the engine to have drained everything queued so far.
    async fn settled(engine: &Engine) {
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
                key,
                endpoint_id: 1,
                command: ClusterCommand::OnOff(OnOffCommand::On),
            } if key == LocalKey::from("lamp-1")
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
                key,
                endpoint_id: 1,
                write: w,
            } if key == LocalKey::from("lamp-1") && w == write
        ));
        running.abort();
    }

    #[tokio::test]
    async fn a_node_announced_by_the_api_is_refused() {
        let (engine, _rx, running) = running_engine();
        let node_id = recorder_id("lamp-1");
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
        stamped_as(
            &engine,
            "recorder",
            Event::NodeAdded {
                node_id,
                node: lamp("lamp-2"),
            },
        )
        .await;
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
        let cluster = Cluster::OnOff(OnOffCluster { on_off: true });

        stamped_as(
            &engine,
            "impostor",
            Event::Report {
                node_id,
                endpoint_id: 1,
                cluster: cluster.clone(),
            },
        )
        .await;
        settled(&engine).await;
        assert!(engine.state_snapshot().nodes[&node_id].endpoints.is_empty());

        engine
            .sender_for("recorder")
            .report(&LocalKey::from("lamp-1"), 1, cluster)
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
            .node_removed(&LocalKey::from("lamp-1"))
            .await
            .expect("engine is running");
        settled(&engine).await;

        assert!(engine.state_snapshot().nodes.is_empty());
        assert_eq!(engine.binding(node_id).unwrap(), None);
        running.abort();
    }

    #[tokio::test]
    async fn an_alias_resolves_before_its_node_is_announced() {
        let engine = engine_with_alias("lamp", "recorder", "lamp-1");

        assert_eq!(engine.resolve("lamp"), Ok(recorder_id("lamp-1")));
        assert!(engine.state_snapshot().nodes.is_empty());
        assert_eq!(
            engine.alias_target("lamp"),
            Some(&NodeKey {
                integration: "recorder".into(),
                local: LocalKey::from("lamp-1"),
            })
        );
    }

    #[tokio::test]
    async fn a_discovered_name_resolves_once_announced() {
        let (engine, _rx, running) = running_engine();
        assert_eq!(
            engine.resolve("living_room_lamp"),
            Err(ResolveError::Unknown("living_room_lamp".into()))
        );

        announce(&engine, named("lamp-1", "Living Room Lamp")).await;

        assert_eq!(
            engine.resolve("living_room_lamp"),
            Ok(recorder_id("lamp-1"))
        );
        running.abort();
    }

    #[tokio::test]
    async fn an_alias_shadows_a_discovered_name() {
        let (engine, _rx, running) = start(engine_with_alias("lamp", "recorder", "lamp-1"));

        announce(&engine, named("lamp-2", "Lamp")).await;

        assert_eq!(engine.resolve("lamp"), Ok(recorder_id("lamp-1")));
        assert_eq!(
            engine.state_snapshot().names["lamp"],
            vec![recorder_id("lamp-2")]
        );
        running.abort();
    }

    #[tokio::test]
    async fn a_rename_moves_the_discovered_name() {
        let (engine, _rx, running) = running_engine();
        announce(&engine, named("lamp-1", "Old")).await;
        announce(&engine, named("lamp-1", "New")).await;

        assert_eq!(
            engine.resolve("old"),
            Err(ResolveError::Unknown("old".into()))
        );
        assert_eq!(engine.resolve("new"), Ok(recorder_id("lamp-1")));
        running.abort();
    }

    #[tokio::test]
    async fn removal_keeps_the_alias_and_frees_the_discovered_name() {
        let (engine, _rx, running) = start(engine_with_alias("lamp", "recorder", "lamp-1"));
        announce(&engine, named("lamp-1", "Lamp")).await;

        engine
            .sender_for("recorder")
            .node_removed(&LocalKey::from("lamp-1"))
            .await
            .expect("engine is running");
        settled(&engine).await;

        assert_eq!(engine.resolve("lamp"), Ok(recorder_id("lamp-1")));
        let state = engine.state_snapshot();
        assert!(state.nodes.is_empty());
        assert!(state.names.is_empty());
        running.abort();
    }

    #[tokio::test]
    async fn two_nodes_discovered_under_one_name_are_ambiguous() {
        let (engine, _rx, running) = running_engine();
        announce(&engine, named("a", "Kitchen")).await;
        announce(&engine, named("b", "Kitchen")).await;

        let mut ids = vec![recorder_id("a"), recorder_id("b")];
        ids.sort_unstable();
        assert_eq!(
            engine.resolve("kitchen"),
            Err(ResolveError::Ambiguous {
                name: "kitchen".into(),
                ids,
            })
        );
        running.abort();
    }

    #[tokio::test]
    async fn an_id_in_hex_resolves_to_itself() {
        let engine = Engine::new();
        let id = recorder_id("lamp-1");
        assert_eq!(engine.resolve(&id.to_string()), Ok(id));
        assert_eq!(
            engine.resolve("nonsense"),
            Err(ResolveError::Unknown("nonsense".into()))
        );
    }
}
