//! Snapcast integration for hearthd.
//!
//! Controls a Snapserver over its raw TCP JSON-RPC control protocol (port
//! 1705). Each group becomes a media-player node and each client a speaker
//! node.
//!
//! Snapserver pushes a notification whenever anything changes but the
//! notifications carry partial state, so every one of them is answered with a
//! fresh `Server.GetStatus` and the result diffed against what was last
//! published. Refreshes are coalesced through a one-slot channel: a volume
//! slider drag produces a burst of notifications, and there is no value in
//! more than one refresh behind the last of them.

use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::client::ClientEvent;
use super::client::SnapcastRpcClient;
use super::config::Config;
use super::mapper;
use super::models::Client;
use super::models::GetStatusResult;
use super::models::Group;
use super::models::Stream;
use crate::engine::FromIntegrationMessage;
use crate::engine::FromIntegrationSender;
use crate::engine::Integration;
use crate::engine::NodeId;
use crate::engine::NodeIdAllocator;
use crate::engine::ToIntegrationMessage;
use crate::matter::Node;

/// Integration name reported to the engine.
const INTEGRATION_NAME: &str = "snapcast";

/// Mutable view of the server, and the node identities derived from it.
#[derive(Default)]
struct Inner {
    /// Snapcast group id -> NodeId.
    group_node_ids: HashMap<String, NodeId>,
    /// Snapcast client id -> NodeId.
    client_node_ids: HashMap<String, NodeId>,
    /// NodeId -> Snapcast group id for command routing.
    node_to_group: HashMap<NodeId, String>,
    /// NodeId -> Snapcast client id for command routing.
    node_to_client: HashMap<NodeId, String>,
    /// Current stream set.
    streams: HashMap<String, Stream>,
    /// Stable index assigned to each stream id.
    stream_indices: HashMap<String, u8>,
    /// Reverse of `stream_indices`, for resolving `SelectInput`.
    stream_by_index: HashMap<u8, String>,
    /// Current group state.
    groups: HashMap<String, Group>,
    /// Current client state.
    clients: HashMap<String, Client>,
    /// Last node published for each id, so a refresh can report what actually
    /// changed instead of re-announcing everything.
    published: HashMap<NodeId, Node>,
}

/// Everything built during `setup` and shared with the background tasks.
struct State {
    client: SnapcastRpcClient,
    to_engine: FromIntegrationSender,
    node_ids: NodeIdAllocator,
    refresh_tx: mpsc::Sender<()>,
    inner: Mutex<Inner>,
}

/// Snapcast integration.
pub struct SnapcastIntegration {
    config: Config,
    state: Option<Arc<State>>,
    tasks: Vec<JoinHandle<()>>,
}

impl SnapcastIntegration {
    /// Create a new integration from configuration.
    ///
    /// Node ids are not allocated here: the engine hands the allocator to
    /// `setup`, and that one is the only one whose ids are unique across
    /// integrations.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: None,
            tasks: Vec::new(),
        }
    }
}

/// Fetch the full server status and publish whatever it changed.
async fn refresh(state: &State) -> Result<(), Box<dyn Error + Send + Sync>> {
    let status: GetStatusResult = state.client.request("Server.GetStatus", ()).await?;
    let status = status.server;

    let messages = {
        let mut inner = state.inner.lock().await;

        inner.streams.clear();
        inner.stream_indices.clear();
        inner.stream_by_index.clear();
        for (idx, stream) in status.streams.iter().enumerate() {
            // More streams than a u8 can index is not a real configuration,
            // and truncating would alias two streams onto one index.
            let Ok(index) = u8::try_from(idx) else {
                warn!("Ignoring Snapcast stream {} beyond index 255", stream.id);
                continue;
            };
            inner.streams.insert(stream.id.clone(), stream.clone());
            inner.stream_indices.insert(stream.id.clone(), index);
            inner.stream_by_index.insert(index, stream.id.clone());
        }

        let mut messages = Vec::new();

        let mut live_groups: HashMap<String, NodeId> = HashMap::new();
        let mut live_clients: HashMap<String, NodeId> = HashMap::new();
        let mut groups = HashMap::new();
        let mut clients = HashMap::new();

        for group in &status.groups {
            let node_id = match inner.group_node_ids.get(&group.id) {
                Some(id) => *id,
                None => state.node_ids.allocate(),
            };
            live_groups.insert(group.id.clone(), node_id);
            groups.insert(group.id.clone(), group.clone());

            let node = mapper::group_node(
                group,
                &inner.streams,
                &inner.stream_indices,
                &mapper::group_entity_id(group),
            );
            publish(&mut inner, &mut messages, node_id, node);

            for client in &group.clients {
                let node_id = match inner.client_node_ids.get(&client.id) {
                    Some(id) => *id,
                    None => state.node_ids.allocate(),
                };
                live_clients.insert(client.id.clone(), node_id);
                clients.insert(client.id.clone(), client.clone());

                let node = mapper::client_node(client, &mapper::client_entity_id(client));
                publish(&mut inner, &mut messages, node_id, node);
            }
        }

        let departed: Vec<NodeId> = inner
            .group_node_ids
            .iter()
            .filter(|(id, _)| !live_groups.contains_key(*id))
            .chain(
                inner
                    .client_node_ids
                    .iter()
                    .filter(|(id, _)| !live_clients.contains_key(*id)),
            )
            .map(|(_, node_id)| *node_id)
            .collect();
        for node_id in departed {
            messages.push(FromIntegrationMessage::NodeRemoved { node_id });
            inner.published.remove(&node_id);
        }

        inner.node_to_group = live_groups.iter().map(|(k, v)| (*v, k.clone())).collect();
        inner.node_to_client = live_clients.iter().map(|(k, v)| (*v, k.clone())).collect();
        inner.group_node_ids = live_groups;
        inner.client_node_ids = live_clients;
        inner.groups = groups;
        inner.clients = clients;

        debug!(
            "Snapcast status applied: {} groups, {} clients, {} streams, {} updates",
            inner.groups.len(),
            inner.clients.len(),
            inner.streams.len(),
            messages.len(),
        );

        messages
    };

    // Sent with the lock released: the engine channel is bounded, so holding
    // it here would stall command handling behind a slow consumer.
    for message in messages {
        if state.to_engine.send(message).await.is_err() {
            return Err("engine channel closed".into());
        }
    }

    Ok(())
}

/// Queue the messages that move a node from its published form to `node`.
///
/// A node the engine has not seen is announced whole; one it already has
/// reports only the clusters whose contents differ, which is what makes the
/// engine emit attribute-change events rather than repeated discovery.
fn publish(
    inner: &mut Inner,
    messages: &mut Vec<FromIntegrationMessage>,
    node_id: NodeId,
    node: Node,
) {
    match inner.published.get(&node_id) {
        // Identity is only carried by NodeAdded, so a device renamed in
        // Snapcast has to be re-announced rather than described by a cluster
        // diff that has no field for it.
        Some(previous) if previous.entity_id != node.entity_id || previous.name != node.name => {
            messages.push(FromIntegrationMessage::NodeAdded {
                node_id,
                node: node.clone(),
            });
        }
        None => messages.push(FromIntegrationMessage::NodeAdded {
            node_id,
            node: node.clone(),
        }),
        Some(previous) => {
            for (endpoint_id, endpoint) in &node.endpoints {
                for (name, cluster) in &endpoint.clusters {
                    let unchanged = previous
                        .endpoints
                        .get(endpoint_id)
                        .and_then(|e| e.clusters.get(name))
                        .is_some_and(|p| p == cluster);
                    if !unchanged {
                        messages.push(FromIntegrationMessage::AttributeChanged {
                            node_id,
                            endpoint_id: *endpoint_id,
                            cluster: cluster.clone(),
                        });
                    }
                }
            }
        }
    }

    inner.published.insert(node_id, node);
}

/// Notifications that mean the published view is now stale.
fn is_state_changing(method: &str) -> bool {
    matches!(
        method,
        "Server.OnUpdate"
            | "Group.OnMute"
            | "Group.OnStreamChanged"
            | "Group.OnNameChanged"
            | "Client.OnConnect"
            | "Client.OnDisconnect"
            | "Client.OnVolumeChanged"
            | "Client.OnNameChanged"
            | "Client.OnLatencyChanged"
            | "Stream.OnUpdate"
            | "Stream.OnProperties"
    )
}

#[async_trait]
impl Integration for SnapcastIntegration {
    fn name(&self) -> &str {
        INTEGRATION_NAME
    }

    async fn setup(
        &mut self,
        tx: FromIntegrationSender,
        node_ids: NodeIdAllocator,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (client, mut events) = SnapcastRpcClient::new(
            self.config.host.clone(),
            self.config.port,
            self.config.reconnect_interval_ms,
        );

        // One slot: a refresh already queued will see everything that
        // happened before it runs, so further requests behind it are noise.
        let (refresh_tx, mut refresh_rx) = mpsc::channel(1);

        let state = Arc::new(State {
            client,
            to_engine: tx,
            node_ids,
            refresh_tx: refresh_tx.clone(),
            inner: Mutex::new(Inner::default()),
        });

        // Started before anything is sent: requests fail fast while
        // disconnected, and the first successful connection is what triggers
        // the initial fetch.
        self.tasks.push(state.client.spawn());

        let event_state = state.clone();
        self.tasks.push(tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    ClientEvent::Connected => {
                        // Resynchronise on every connection, not just the
                        // first: anything that changed while disconnected was
                        // never notified.
                        let _ = event_state.refresh_tx.try_send(());
                    }
                    ClientEvent::Notification(n) if is_state_changing(&n.method) => {
                        let _ = event_state.refresh_tx.try_send(());
                    }
                    ClientEvent::Notification(n) => {
                        debug!("Ignoring Snapcast notification: {}", n.method);
                    }
                }
            }
        }));

        let refresh_state = state.clone();
        let retry_interval = Duration::from_millis(self.config.reconnect_interval_ms);
        self.tasks.push(tokio::spawn(async move {
            while refresh_rx.recv().await.is_some() {
                // Retry until one succeeds. Only a connection or a server-side
                // change schedules a refresh, so a failure that leaves the
                // connection up has nothing to schedule the next attempt: an
                // idle server would leave the integration publishing nothing
                // at all while its logs look healthy.
                while let Err(e) = refresh(&refresh_state).await {
                    warn!("Snapcast refresh failed, retrying: {e}");
                    tokio::time::sleep(retry_interval).await;
                }
            }
        }));

        self.state = Some(state);
        info!(
            "Snapcast integration started for {}:{}",
            self.config.host, self.config.port
        );
        Ok(())
    }

    async fn handle_message(
        &mut self,
        msg: ToIntegrationMessage,
    ) -> Result<(), Box<dyn Error + Send>> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "Snapcast integration is not set up",
                ))
            })?;

        match msg {
            ToIntegrationMessage::InvokeCommand {
                node_id,
                endpoint_id,
                command,
            } => {
                if endpoint_id != mapper::SNAPCAST_ENDPOINT {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("Unknown endpoint {endpoint_id} on node {node_id}"),
                    )));
                }

                let (method, params) = {
                    let inner = state.inner.lock().await;
                    let ctx = mapper::CommandContext {
                        node_to_group: &inner.node_to_group,
                        node_to_client: &inner.node_to_client,
                        groups: &inner.groups,
                        clients: &inner.clients,
                        stream_by_index: &inner.stream_by_index,
                    };
                    mapper::command_to_rpc(node_id, &command, &ctx).ok_or_else(
                        || -> Box<dyn Error + Send> {
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                format!(
                                    "No Snapcast command mapping for node {node_id} command {command:?}"
                                ),
                            ))
                        },
                    )?
                };

                debug!("Sending Snapcast RPC {method} {params}");
                state
                    .client
                    .request::<_, serde_json::Value>(method, params)
                    .await
                    .map_err(|e| -> Box<dyn Error + Send> {
                        Box::new(std::io::Error::other(format!("{method} failed: {e}")))
                    })?;

                // Snapserver notifies on change, but asking directly means the
                // new state is published even if that notification is missed.
                let _ = state.refresh_tx.try_send(());
            }
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
        for task in self.tasks.drain(..) {
            task.abort();
        }
        self.state = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matter::Cluster;
    use crate::matter::Endpoint;
    use crate::matter::OnOffCluster;

    fn node(entity_id: &str, name: &str, on_off: bool) -> Node {
        let mut endpoint = Endpoint::default();
        endpoint.clusters.insert(
            crate::matter::CLUSTER_NAME_ON_OFF.to_string(),
            Cluster::OnOff(OnOffCluster { on_off }),
        );
        let mut endpoints = HashMap::new();
        endpoints.insert(mapper::SNAPCAST_ENDPOINT, endpoint);
        Node {
            entity_id: entity_id.to_string(),
            integration: INTEGRATION_NAME.to_string(),
            name: Some(name.to_string()),
            endpoints,
        }
    }

    #[test]
    fn a_node_the_engine_has_not_seen_is_announced_whole() {
        let mut inner = Inner::default();
        let mut messages = Vec::new();
        let id = NodeId::from_raw(1);

        publish(&mut inner, &mut messages, id, node("speaker.a", "A", true));

        assert!(matches!(
            messages.as_slice(),
            [FromIntegrationMessage::NodeAdded { node_id, .. }] if *node_id == id
        ));
    }

    #[test]
    fn republishing_an_identical_node_says_nothing() {
        // Snapserver notifies on any change and the whole status is refetched
        // each time, so most refreshes find nothing new. Re-announcing them
        // would turn every unrelated volume change into an event for every
        // node on the server.
        let mut inner = Inner::default();
        let mut messages = Vec::new();
        let id = NodeId::from_raw(1);

        publish(&mut inner, &mut messages, id, node("speaker.a", "A", true));
        messages.clear();
        publish(&mut inner, &mut messages, id, node("speaker.a", "A", true));

        assert!(messages.is_empty());
    }

    #[test]
    fn only_the_clusters_that_differ_are_reported() {
        let mut inner = Inner::default();
        let mut messages = Vec::new();
        let id = NodeId::from_raw(1);

        publish(&mut inner, &mut messages, id, node("speaker.a", "A", true));
        messages.clear();
        publish(&mut inner, &mut messages, id, node("speaker.a", "A", false));

        match messages.as_slice() {
            [
                FromIntegrationMessage::AttributeChanged {
                    node_id,
                    endpoint_id,
                    cluster: Cluster::OnOff(c),
                },
            ] => {
                assert_eq!(*node_id, id);
                assert_eq!(*endpoint_id, mapper::SNAPCAST_ENDPOINT);
                assert!(!c.on_off);
            }
            other => panic!("expected one OnOff attribute change, got {other:?}"),
        }
    }

    #[test]
    fn a_renamed_node_is_reannounced() {
        // An attribute change has no field for the name or the entity id, so
        // a device renamed in Snapcast can only be reported by announcing it
        // again under the same node id.
        let mut inner = Inner::default();
        let mut messages = Vec::new();
        let id = NodeId::from_raw(1);

        publish(&mut inner, &mut messages, id, node("speaker.a", "A", true));
        messages.clear();
        publish(
            &mut inner,
            &mut messages,
            id,
            node("speaker.kitchen", "Kitchen", true),
        );

        match messages.as_slice() {
            [FromIntegrationMessage::NodeAdded { node_id, node }] => {
                assert_eq!(*node_id, id);
                assert_eq!(node.entity_id, "speaker.kitchen");
                assert_eq!(node.name.as_deref(), Some("Kitchen"));
            }
            other => panic!("expected a re-announcement, got {other:?}"),
        }
    }

    #[test]
    fn notifications_that_change_state_are_told_from_those_that_do_not() {
        assert!(is_state_changing("Client.OnVolumeChanged"));
        assert!(is_state_changing("Group.OnStreamChanged"));
        assert!(is_state_changing("Stream.OnProperties"));
        assert!(is_state_changing("Server.OnUpdate"));
        assert!(!is_state_changing("Stream.OnSomethingElse"));
    }
}
