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
use super::integration::FromIntegrationReceiver;
use super::integration::FromIntegrationSender;
use super::integration::Integration;
use super::integration::ToIntegrationSender;
use super::message::FromIntegrationMessage;
use super::message::ToIntegrationMessage;
use super::runner::AutomationRunner;
use super::state::State;
use crate::engine::IntegrationContext;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;
use crate::matter::NodeId;

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

    /// Receive messages from integrations (events)
    message_rx: Mutex<FromIntegrationReceiver>,

    /// Sender for integrations to report events back to the engine
    message_tx: FromIntegrationSender,

    /// Handles for integration tasks
    integration_handles: Vec<JoinHandle<()>>,

    /// Optional automation runner. When set, the engine calls
    /// `runner.dispatch(...)` on each `AttributeChanged` event from
    /// inside its own task; the runner is responsible for any task
    /// spawning needed to actually execute automations.
    ///
    /// Held in a `std::sync::RwLock` rather than `arc_swap` because
    /// trait objects don't satisfy `arc_swap::RefCnt`'s `Sized` bound.
    /// `set_runner` is expected to be called at most a handful of times
    /// (typically once at startup), so the lock is not on a hot path.
    runner: std::sync::RwLock<Option<Arc<dyn AutomationRunner>>>,
}

/// Capacity for the integration→engine message channel
/// Provides backpressure when integrations send faster than the engine can process
const FROM_INTEGRATION_CHANNEL_SIZE: usize = 1024;

impl Engine {
    /// Create a new Engine instance
    pub fn new() -> Self {
        let (message_tx, message_rx) = mpsc::channel(FROM_INTEGRATION_CHANNEL_SIZE);
        Self {
            state: ArcSwap::new(Arc::default()),
            node_integration_map: std::sync::Mutex::new(HashMap::new()),
            integration_channels: HashMap::new(),
            message_rx: Mutex::new(message_rx),
            message_tx,
            integration_handles: Vec::new(),
            runner: std::sync::RwLock::new(None),
        }
    }

    /// Install (or replace) the automation runner. Subsequent events
    /// will be dispatched to it.
    pub fn set_runner(&self, runner: Arc<dyn AutomationRunner>) {
        *self.runner.write().expect("runner lock poisoned") = Some(runner);
    }

    /// Snapshot the current automation runner, if any.
    fn current_runner(&self) -> Option<Arc<dyn AutomationRunner>> {
        self.runner
            .read()
            .expect("runner lock poisoned")
            .as_ref()
            .map(Arc::clone)
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
        let from_integration_tx = self.message_tx.clone();

        self.integration_channels
            .insert(name.clone(), to_integration_tx);

        // Spawn integration task
        let handle = tokio::spawn(async move {
            // Setup integration (gives it the sender for events)
            if let Err(e) = integration.setup(from_integration_tx).await {
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

    /// Send a command to an integration.
    ///
    /// Routes the command to the integration that owns the target node.
    pub fn send_command(&self, msg: ToIntegrationMessage) -> Result<(), Box<dyn Error + Send>> {
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
    /// Processes incoming events from integrations and updates state.
    pub async fn run(&self) -> Result<(), Box<dyn Error + Send>> {
        info!("Engine starting");

        // Main event loop - only receives FromIntegration messages
        let mut rx = self.message_rx.lock().await;
        while let Some(msg) = rx.recv().await {
            if let Err(e) = self.handle_event(msg).await {
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

    /// Invoke a Matter cluster command on a node's endpoint.
    pub fn invoke_command(
        &self,
        node_id: NodeId,
        endpoint_id: EndpointId,
        command: ClusterCommand,
    ) -> Result<(), Box<dyn Error + Send>> {
        self.send_command(ToIntegrationMessage::InvokeCommand {
            node_id,
            endpoint_id,
            command,
        })
    }

    /// Handle an event from an integration
    async fn handle_event(&self, msg: FromIntegrationMessage) -> Result<(), Box<dyn Error + Send>> {
        match msg {
            FromIntegrationMessage::NodeAdded { node_id, node } => {
                info!(
                    "Node added: {} ({}) from {}",
                    node_id, node.entity_id, node.integration
                );

                if let Ok(mut map) = self.node_integration_map.lock() {
                    map.insert(node_id, node.integration.clone());
                }

                {
                    let mut state = State::clone(&self.state.load());
                    state.by_entity_id.insert(node.entity_id.clone(), node_id);
                    state.nodes.insert(node_id, node);
                    self.state.store(Arc::new(state));
                }
            }
            FromIntegrationMessage::NodeRemoved { node_id } => {
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
            FromIntegrationMessage::AttributeChanged {
                node_id,
                endpoint_id,
                cluster,
            } => {
                info!(
                    "Attribute changed: node={} endpoint={} cluster={}",
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
                            .insert(cluster.name().to_string(), cluster.clone());
                    }
                    self.state.store(Arc::new(state));
                }

                let event = match cluster {
                    Cluster::OnOff(attributes) => Event::OnOffChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::LevelControl(attributes) => Event::LevelControlChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::OccupancySensing(attributes) => Event::OccupancySensingChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                };

                if let Some(runner) = self.current_runner() {
                    let snapshot = self.state.load_full();
                    runner.dispatch(&event, &snapshot);
                }
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
    use std::sync::Mutex;

    use super::*;
    use crate::matter::Endpoint;
    use crate::matter::Node;
    use crate::matter::OccupancySensingCluster;

    /// Recording fake: stores every event it sees in order.
    #[derive(Default)]
    struct RecordingRunner {
        events: Mutex<Vec<Event>>,
    }

    impl AutomationRunner for RecordingRunner {
        fn dispatch(&self, event: &Event, _state: &Arc<State>) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    fn fake_motion_node() -> Node {
        Node {
            id: 7,
            entity_id: "binary_sensor.kitchen_motion".into(),
            integration: "test".into(),
            name: None,
            endpoints: HashMap::from([(1u16, Endpoint::default())]),
        }
    }

    #[tokio::test]
    async fn dispatch_fires_on_attribute_changed() {
        let engine = Engine::new();

        // Seed the state with a known node so AttributeChanged has a
        // target to merge into.
        {
            let mut state = State::clone(&engine.state.load());
            let node = fake_motion_node();
            let id = node.id;
            state.by_entity_id.insert(node.entity_id.clone(), id);
            state.nodes.insert(id, node);
            engine.state.store(Arc::new(state));
        }

        let recorder = Arc::new(RecordingRunner::default());
        engine.set_runner(recorder.clone());

        engine
            .handle_event(FromIntegrationMessage::AttributeChanged {
                node_id: 7,
                endpoint_id: 1,
                cluster: Cluster::OccupancySensing(OccupancySensingCluster { occupancy: true }),
            })
            .await
            .expect("handle_event");

        let events = recorder.events.lock().unwrap();
        assert_eq!(events.len(), 1, "expected one dispatched event");
        match &events[0] {
            Event::OccupancySensingChanged {
                node_id,
                endpoint_id,
                attributes,
            } => {
                assert_eq!(*node_id, 7);
                assert_eq!(*endpoint_id, 1);
                assert!(attributes.occupancy);
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn dispatch_is_a_noop_without_runner() {
        // Without a runner installed, AttributeChanged must still update
        // state and return Ok — we just don't fan it out anywhere.
        let engine = Engine::new();
        let node = fake_motion_node();
        let id = node.id;
        {
            let mut state = State::clone(&engine.state.load());
            state.by_entity_id.insert(node.entity_id.clone(), id);
            state.nodes.insert(id, node);
            engine.state.store(Arc::new(state));
        }

        engine
            .handle_event(FromIntegrationMessage::AttributeChanged {
                node_id: 7,
                endpoint_id: 1,
                cluster: Cluster::OccupancySensing(OccupancySensingCluster { occupancy: true }),
            })
            .await
            .expect("handle_event");

        let cluster = engine
            .state_snapshot()
            .nodes
            .get(&7)
            .and_then(|n| n.endpoints.get(&1))
            .and_then(|e| e.clusters.get("OccupancySensing"))
            .cloned()
            .expect("cluster should be present");
        match cluster {
            Cluster::OccupancySensing(c) => assert!(c.occupancy),
            other => panic!("unexpected cluster {:?}", other),
        }
    }
}
