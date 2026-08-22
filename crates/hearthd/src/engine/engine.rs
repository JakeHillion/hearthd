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
use super::state::State;
use crate::engine::IntegrationContext;
use crate::engine::NodeId;
use crate::engine::registry::CollisionPolicy;
use crate::engine::registry::NodeRegistry;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;

/// hearthd engine
///
/// This structure handles the flow of events, applying automations to them, sending them to the
/// correct integration, and maintaining a view of the world with State.
pub struct Engine {
    /// Centralized state snapshot (readers load the Arc, writer stores a new one)
    state: ArcSwap<State>,

    /// Communication channels to integrations (for commands)
    integration_channels: HashMap<String, ToIntegrationSender>,

    /// Receive messages from integrations (events)
    message_rx: Mutex<FromIntegrationReceiver>,

    /// Sender for integrations to report events back to the engine
    message_tx: FromIntegrationSender,

    /// Handles for integration tasks
    integration_handles: Vec<JoinHandle<()>>,

    /// Source of node identity for every integration: node ids, entity ids
    /// and the record of who owns what. One per engine, which is what makes
    /// all three unique.
    nodes: NodeRegistry,
}

/// Capacity for the integration→engine message channel
/// Provides backpressure when integrations send faster than the engine can process
const FROM_INTEGRATION_CHANNEL_SIZE: usize = 1024;

impl Engine {
    /// Create a new Engine instance.
    ///
    /// `policy` decides what happens when two nodes ask for one entity_id;
    /// see [`CollisionPolicy`]. It is fixed for the life of the engine so that
    /// no node can be registered before the rule that governs it is known.
    pub fn new(policy: CollisionPolicy) -> Self {
        let (message_tx, message_rx) = mpsc::channel(FROM_INTEGRATION_CHANNEL_SIZE);
        Self {
            state: ArcSwap::new(Arc::default()),
            integration_channels: HashMap::new(),
            message_rx: Mutex::new(message_rx),
            nodes: NodeRegistry::new(message_tx.clone(), policy),
            message_tx,
            integration_handles: Vec::new(),
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
        let from_integration_tx = self.message_tx.clone();
        let nodes = self.nodes.for_integration(&name);

        self.integration_channels
            .insert(name.clone(), to_integration_tx);

        // Spawn integration task
        let handle = tokio::spawn(async move {
            // Setup integration (gives it the sender for events)
            if let Err(e) = integration.setup(from_integration_tx, nodes).await {
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

        let integration_name =
            self.nodes
                .owner(node_id)
                .ok_or_else(|| -> Box<dyn Error + Send> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("No integration found for node: {}", node_id),
                    ))
                })?;

        let tx = self
            .integration_channels
            .get(&*integration_name)
            .ok_or_else(|| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Integration channel not found: {}", integration_name),
                ))
            })?;

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
            // Both node messages are projections of decisions the registry has
            // already made: it assigned the id, the name and the owner before
            // the message was sent, so nothing here chooses anything.
            FromIntegrationMessage::NodeAdded { node_id, node } => {
                info!(
                    "Node added: {} ({}) from {}",
                    node_id, node.entity_id, node.integration
                );

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
            FromIntegrationMessage::NodeRemoved { node_id } => {
                info!("Node removed: {}", node_id);

                {
                    let mut state = State::clone(&self.state.load());
                    if let Some(node) = state.nodes.remove(&node_id) {
                        // Only if the name still resolves to the node being
                        // removed. A node that was removed and re-registered
                        // under the same name would otherwise have its
                        // successor unindexed by this late removal.
                        if state.by_entity_id.get(&node.entity_id) == Some(&node_id) {
                            state.by_entity_id.remove(&node.entity_id);
                        }
                    }
                    self.state.store(Arc::new(state));
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

                let _event = match cluster {
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
                    Cluster::ColorControl(attributes) => Event::ColorControlChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::TemperatureMeasurement(attributes) => {
                        Event::TemperatureMeasurementChanged {
                            node_id,
                            endpoint_id,
                            attributes,
                        }
                    }
                    Cluster::PressureMeasurement(attributes) => Event::PressureMeasurementChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::RelativeHumidityMeasurement(attributes) => {
                        Event::RelativeHumidityMeasurementChanged {
                            node_id,
                            endpoint_id,
                            attributes,
                        }
                    }
                    Cluster::OccupancySensing(attributes) => Event::OccupancySensingChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::BooleanState(attributes) => Event::BooleanStateChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::Thermostat(attributes) => Event::ThermostatChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::FanControl(attributes) => Event::FanControlChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::DehumidificationControl(attributes) => {
                        Event::DehumidificationControlChanged {
                            node_id,
                            endpoint_id,
                            attributes,
                        }
                    }
                    Cluster::ThermostatUserInterfaceConfiguration(attributes) => {
                        Event::ThermostatUserInterfaceConfigurationChanged {
                            node_id,
                            endpoint_id,
                            attributes,
                        }
                    }
                    Cluster::PowerSource(attributes) => Event::PowerSourceChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::ElectricalPowerMeasurement(attributes) => {
                        Event::ElectricalPowerMeasurementChanged {
                            node_id,
                            endpoint_id,
                            attributes,
                        }
                    }
                    Cluster::ModeSelect(attributes) => Event::ModeSelectChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::MediaPlayback(attributes) => Event::MediaPlaybackChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::MediaInput(attributes) => Event::MediaInputChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::WindMeasurement(attributes) => Event::WindMeasurementChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::CloudCover(attributes) => Event::CloudCoverChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::DewPoint(attributes) => Event::DewPointChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::UvIndex(attributes) => Event::UvIndexChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::Precipitation(attributes) => Event::PrecipitationChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                    Cluster::WeatherCondition(attributes) => Event::WeatherConditionChanged {
                        node_id,
                        endpoint_id,
                        attributes,
                    },
                };
                // TODO: Trigger automations based on attribute-changed event
            }
        }
        Ok(())
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(CollisionPolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matter::Node;

    fn node(entity_id: &str) -> Node {
        Node {
            entity_id: entity_id.to_string(),
            integration: "mqtt".to_string(),
            name: None,
            endpoints: HashMap::new(),
        }
    }

    /// The registry frees a name as soon as its holder is deregistered, so a
    /// re-registration can reach the engine before the `NodeRemoved` that
    /// freed the name does. The removal must not then unindex a name that now
    /// belongs to someone else — that leaves a live node addressable by
    /// nothing.
    #[tokio::test]
    async fn a_late_removal_does_not_unindex_a_name_it_no_longer_holds() {
        let engine = Engine::new(CollisionPolicy::Reject);
        let first = NodeId::from_raw(1);
        let second = NodeId::from_raw(2);

        for msg in [
            FromIntegrationMessage::NodeAdded {
                node_id: first,
                node: node("light.a"),
            },
            FromIntegrationMessage::NodeAdded {
                node_id: second,
                node: node("light.a"),
            },
            FromIntegrationMessage::NodeRemoved { node_id: first },
        ] {
            engine.handle_event(msg).await.expect("event should apply");
        }

        assert_eq!(engine.resolve_entity_id("light.a"), Some(second));
        assert!(engine.state_snapshot().nodes.contains_key(&second));
    }
}
