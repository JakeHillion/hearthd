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
use super::client::RpcClientError;
use super::client::SnapcastRpcClient;
use super::config::Config;
use super::mapper;
use super::models::Client;
use super::models::GetStatusResult;
use super::models::Group;
use super::models::Stream;
use crate::engine::Announcement;
use crate::engine::FromIntegrationMessage;
use crate::engine::FromIntegrationSender;
use crate::engine::Integration;
use crate::engine::IntegrationRegistry;
use crate::engine::NodeId;
use crate::engine::RegisteredNode;
use crate::engine::ToIntegrationMessage;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;
use crate::matter::Node;

/// Integration name reported to the engine.
const INTEGRATION_NAME: &str = "snapcast";

/// Why a refresh could not be applied.
#[derive(Debug, thiserror::Error)]
enum RefreshError {
    #[error("Server.GetStatus failed")]
    GetStatus(#[from] RpcClientError),

    /// The engine is gone, so there is nobody left to publish to.
    #[error("engine channel closed")]
    EngineGone,
}

/// Why a command from the engine could not be carried out.
#[derive(Debug, thiserror::Error)]
enum CommandError {
    #[error("snapcast integration is not set up")]
    NotSetUp,

    #[error("unknown endpoint {endpoint_id} on node {node_id}")]
    UnknownEndpoint {
        node_id: NodeId,
        endpoint_id: EndpointId,
    },

    #[error("no snapcast command mapping for node {node_id} command {command:?}")]
    Unmapped {
        node_id: NodeId,
        command: ClusterCommand,
    },

    #[error("{method} failed")]
    Rpc {
        method: &'static str,
        #[source]
        source: RpcClientError,
    },
}

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
    /// Position of each stream id in the server's stream list, used as the
    /// `MediaInput` index. Positional rather than durable: adding or removing a
    /// stream renumbers the ones after it, and the whole `MediaInput` cluster is
    /// republished when it does.
    stream_indices: HashMap<String, u8>,
    /// Reverse of `stream_indices`, for resolving `SelectInput`.
    stream_by_index: HashMap<u8, String>,
    /// Current group state.
    groups: HashMap<String, Group>,
    /// Current client state.
    clients: HashMap<String, Client>,
    /// Last node published for each id, so a refresh can report what actually
    /// changed instead of re-announcing everything. The `entity_id` recorded
    /// here is the one the registry assigned, not the one derived from the
    /// Snapcast name, so that a node admitted under a suffix is not diffed
    /// against a name it never held and re-announced on every refresh.
    published: HashMap<NodeId, Node>,
    /// Registration handle per node: the only way to announce or remove one.
    registrations: HashMap<NodeId, RegisteredNode>,
}

/// A Snapcast object that has just appeared and has yet to be registered.
///
/// Registration is async and sends to the engine, so it happens once the
/// status lock is released; this records which object the resulting node id
/// belongs to.
enum Subject {
    Group(String),
    Client(String),
}

/// What a refresh decided to tell the engine, to be sent once the lock is
/// released.
#[derive(Default)]
struct Outgoing {
    /// Re-announcements of nodes that are already registered.
    announcements: Vec<Announcement>,
    /// Attribute updates for nodes whose identity has not changed.
    updates: Vec<FromIntegrationMessage>,
    /// Objects seen for the first time, still to be registered.
    new: Vec<(Subject, Node)>,
    /// Handles for nodes that have gone away.
    departed: Vec<RegisteredNode>,
}

/// Everything built during `setup` and shared with the background tasks.
struct State {
    client: SnapcastRpcClient,
    to_engine: FromIntegrationSender,
    nodes: IntegrationRegistry,
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
    /// No nodes are declared here: the engine hands the registry to `setup`,
    /// and registering through it is what makes a node's id and entity id
    /// unique across integrations.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: None,
            tasks: Vec::new(),
        }
    }

    /// Carry out one message from the engine.
    async fn invoke(&self, msg: ToIntegrationMessage) -> Result<(), CommandError> {
        let state = self.state.as_ref().ok_or(CommandError::NotSetUp)?;

        match msg {
            ToIntegrationMessage::InvokeCommand {
                node_id,
                endpoint_id,
                command,
            } => {
                if endpoint_id != mapper::SNAPCAST_ENDPOINT {
                    return Err(CommandError::UnknownEndpoint {
                        node_id,
                        endpoint_id,
                    });
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
                    mapper::command_to_rpc(node_id, &command, &ctx).ok_or_else(|| {
                        CommandError::Unmapped {
                            node_id,
                            command: command.clone(),
                        }
                    })?
                };

                debug!("Sending Snapcast RPC {method} {params}");
                state
                    .client
                    .request::<_, serde_json::Value>(method, params)
                    .await
                    .map_err(|source| CommandError::Rpc { method, source })?;

                // Snapserver notifies on change, but asking directly means the
                // new state is published even if that notification is missed.
                let _ = state.refresh_tx.try_send(());
            }
        }
        Ok(())
    }
}

/// Fetch the full server status and publish whatever it changed.
async fn refresh(state: &State) -> Result<(), RefreshError> {
    let status: GetStatusResult = state.client.request("Server.GetStatus", ()).await?;
    let status = status.server;

    let outgoing = {
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

        let mut outgoing = Outgoing::default();

        let mut live_groups: HashMap<String, NodeId> = HashMap::new();
        let mut live_clients: HashMap<String, NodeId> = HashMap::new();
        let mut groups = HashMap::new();
        let mut clients = HashMap::new();

        for group in &status.groups {
            let node = mapper::group_node(
                group,
                &inner.streams,
                &inner.stream_indices,
                &mapper::group_entity_id(group),
            );
            groups.insert(group.id.clone(), group.clone());

            match inner.group_node_ids.get(&group.id).copied() {
                Some(node_id) => {
                    live_groups.insert(group.id.clone(), node_id);
                    publish(&mut inner, &mut outgoing, node_id, node);
                }
                // Registered outside the lock, and only then does it have a
                // node id to be live under.
                None => outgoing.new.push((Subject::Group(group.id.clone()), node)),
            }

            for client in &group.clients {
                let node = mapper::client_node(client, &mapper::client_entity_id(client));
                clients.insert(client.id.clone(), client.clone());

                match inner.client_node_ids.get(&client.id).copied() {
                    Some(node_id) => {
                        live_clients.insert(client.id.clone(), node_id);
                        publish(&mut inner, &mut outgoing, node_id, node);
                    }
                    None => outgoing
                        .new
                        .push((Subject::Client(client.id.clone()), node)),
                }
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
            inner.published.remove(&node_id);
            // The handle is what says the node is gone, and dropping it
            // silently would leave the node in engine state, so it is carried
            // out of here and consumed by `remove()`.
            if let Some(registration) = inner.registrations.remove(&node_id) {
                outgoing.departed.push(registration);
            }
        }

        inner.node_to_group = live_groups.iter().map(|(k, v)| (*v, k.clone())).collect();
        inner.node_to_client = live_clients.iter().map(|(k, v)| (*v, k.clone())).collect();
        inner.group_node_ids = live_groups;
        inner.client_node_ids = live_clients;
        inner.groups = groups;
        inner.clients = clients;

        debug!(
            "Snapcast status applied: {} groups, {} clients, {} streams, {} updates, {} new, {} departed",
            inner.groups.len(),
            inner.clients.len(),
            inner.streams.len(),
            outgoing.announcements.len() + outgoing.updates.len(),
            outgoing.new.len(),
            outgoing.departed.len(),
        );

        outgoing
    };

    // Everything below runs with the lock released: the engine channel is
    // bounded, so holding it here would stall command handling behind a slow
    // consumer, and registering sends on that channel too.
    for announcement in outgoing.announcements {
        announcement.send().await;
    }
    for message in outgoing.updates {
        if state.to_engine.send(message).await.is_err() {
            return Err(RefreshError::EngineGone);
        }
    }
    for registration in outgoing.departed {
        registration.remove().await;
    }

    for (subject, node) in outgoing.new {
        let registration = match state.nodes.register(node.clone()).await {
            Ok(registration) => registration,
            // One unusable name costs one device, not the whole server. The
            // object stays unregistered, so the next refresh tries again.
            Err(e) => {
                warn!("Cannot register Snapcast node {}: {}", node.entity_id, e);
                continue;
            }
        };
        let node_id = registration.node_id();

        // Record the name the registry assigned rather than the one asked
        // for, so the next refresh diffs against what the engine was actually
        // told.
        let mut node = node;
        node.entity_id = registration.entity_id().to_string();

        // Registering announces the node, so between that and this lock the
        // engine knows a node the routing tables do not. A command arriving in
        // that window is refused rather than misrouted, and the entry is
        // permanent once written, so the window closes on its own.
        let mut inner = state.inner.lock().await;
        match subject {
            Subject::Group(id) => {
                inner.node_to_group.insert(node_id, id.clone());
                inner.group_node_ids.insert(id, node_id);
            }
            Subject::Client(id) => {
                inner.node_to_client.insert(node_id, id.clone());
                inner.client_node_ids.insert(id, node_id);
            }
        }
        inner.published.insert(node_id, node);
        inner.registrations.insert(node_id, registration);
    }

    Ok(())
}

/// Queue whatever moves an already-registered node from its published form to
/// `node`.
///
/// A node whose identity changed is re-announced whole; otherwise only the
/// clusters whose contents differ are reported, which is what makes the engine
/// emit attribute-change events rather than repeated discovery.
///
/// Nodes seen for the first time are not handled here: they have no id yet,
/// and acquiring one means registering, which cannot happen under this lock.
fn publish(inner: &mut Inner, outgoing: &mut Outgoing, node_id: NodeId, mut node: Node) {
    // Split so the registration and the published copy can be held at once:
    // the rename below needs the handle mutably while the diff reads the copy.
    let Inner {
        published,
        registrations,
        ..
    } = inner;

    let Some(registration) = registrations.get_mut(&node_id) else {
        warn!("No registration for Snapcast node {node_id}; dropping update");
        return;
    };

    // An entity id derived from a Snapcast name changes when the operator
    // renames the device, and the name is a reservation in a keyspace shared
    // with every other integration — so moving it is the registry's decision,
    // not something that can be stamped onto the node here.
    if registration.entity_id() != node.entity_id {
        if let Err(e) = registration.rename(&node.entity_id) {
            warn!(
                "Cannot rename Snapcast node {} to '{}': {}",
                node_id, node.entity_id, e
            );
        }
    }
    // Whatever the rename settled on, including a refusal that left the old
    // name in place. Recording the requested name instead would make the next
    // refresh see an identity change that never happened, every time.
    node.entity_id = registration.entity_id().to_string();

    match published.get(&node_id) {
        // Identity is only carried by NodeAdded, so a renamed device has to be
        // re-announced rather than described by a cluster diff that has no
        // field for it.
        Some(previous) if previous.entity_id != node.entity_id || previous.name != node.name => {
            outgoing
                .announcements
                .push(registration.announce(node.clone()));
        }
        None => outgoing
            .announcements
            .push(registration.announce(node.clone())),
        Some(previous) => {
            for (endpoint_id, endpoint) in &node.endpoints {
                for (name, cluster) in &endpoint.clusters {
                    let unchanged = previous
                        .endpoints
                        .get(endpoint_id)
                        .and_then(|e| e.clusters.get(name))
                        .is_some_and(|p| p == cluster);
                    if !unchanged {
                        outgoing
                            .updates
                            .push(FromIntegrationMessage::AttributeChanged {
                                node_id,
                                endpoint_id: *endpoint_id,
                                cluster: cluster.clone(),
                            });
                    }
                }
            }
        }
    }

    published.insert(node_id, node);
}

#[async_trait]
impl Integration for SnapcastIntegration {
    fn name(&self) -> &str {
        INTEGRATION_NAME
    }

    async fn setup(
        &mut self,
        tx: FromIntegrationSender,
        nodes: IntegrationRegistry,
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
            nodes,
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
                    ClientEvent::Notification(n) => {
                        // Any notification schedules a refresh, rather than
                        // only a known-interesting subset. The refresh is
                        // coalesced and its result diffed, so one that touched
                        // nothing hearthd models costs a Server.GetStatus and
                        // publishes nothing. An allowlist would have to grow an
                        // entry for every notification Snapserver ever adds,
                        // and would silently ignore them until it did.
                        debug!("Snapcast notification: {}", n.method);
                        let _ = event_state.refresh_tx.try_send(());
                    }
                }
            }
        }));

        let refresh_state = state.clone();
        let retry_interval = Duration::from_millis(self.config.reconnect_interval_ms);
        self.tasks.push(tokio::spawn(async move {
            'refresh: while refresh_rx.recv().await.is_some() {
                // Retry until one succeeds. Only a connection or a server-side
                // change schedules a refresh, so a failure that leaves the
                // connection up has nothing to schedule the next attempt: an
                // idle server would leave the integration publishing nothing
                // at all while its logs look healthy.
                while let Err(e) = refresh(&refresh_state).await {
                    // The exception: there is no publishing to be done once
                    // the engine is gone, so retrying that would spin for as
                    // long as the process lived.
                    if matches!(e, RefreshError::EngineGone) {
                        debug!("Snapcast refresh stopping: {e}");
                        break 'refresh;
                    }
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
        // Boxed once here rather than at every `?`: `Box<dyn Error + Send>`
        // has no blanket `From` impl, so a typed error inside keeps the body
        // free of per-site boxing.
        self.invoke(msg)
            .await
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(e) })
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

    /// An `Inner` holding one registered node, plus the channel the registry
    /// announces on. `published` is left empty, so the node is in the state a
    /// freshly registered one is in: known to the registry, not yet diffed.
    async fn registered(
        entity_id: &str,
    ) -> (Inner, NodeId, mpsc::Receiver<FromIntegrationMessage>) {
        let (tx, mut rx) = mpsc::channel(16);
        let registry = IntegrationRegistry::for_test(INTEGRATION_NAME, tx);

        let registration = registry
            .register(node(entity_id, "seed", true))
            .await
            .expect("the first claim on a name is always free");
        let node_id = registration.node_id();
        // The NodeAdded that registering sends, so the tests below see only
        // what `publish` produced.
        let _ = rx.recv().await;

        let inner = Inner {
            registrations: HashMap::from([(node_id, registration)]),
            ..Default::default()
        };
        (inner, node_id, rx)
    }

    /// Everything an `Outgoing` would put on the wire, in the order `refresh`
    /// sends it. Announcements are opaque, so they are read back off the
    /// channel rather than inspected in place.
    async fn drain(
        outgoing: Outgoing,
        rx: &mut mpsc::Receiver<FromIntegrationMessage>,
    ) -> Vec<FromIntegrationMessage> {
        let mut sent = Vec::new();
        for announcement in outgoing.announcements {
            announcement.send().await;
            sent.push(rx.recv().await.expect("an announcement should arrive"));
        }
        sent.extend(outgoing.updates);
        sent
    }

    #[tokio::test]
    async fn a_node_the_engine_has_not_seen_is_announced_whole() {
        let (mut inner, id, mut rx) = registered("speaker.a").await;
        let mut outgoing = Outgoing::default();

        publish(&mut inner, &mut outgoing, id, node("speaker.a", "A", true));

        assert!(matches!(
            drain(outgoing, &mut rx).await.as_slice(),
            [FromIntegrationMessage::NodeAdded { node_id, .. }] if *node_id == id
        ));
    }

    #[tokio::test]
    async fn republishing_an_identical_node_says_nothing() {
        // Snapserver notifies on any change and the whole status is refetched
        // each time, so most refreshes find nothing new. Re-announcing them
        // would turn every unrelated volume change into an event for every
        // node on the server.
        let (mut inner, id, mut rx) = registered("speaker.a").await;

        let mut first = Outgoing::default();
        publish(&mut inner, &mut first, id, node("speaker.a", "A", true));
        drain(first, &mut rx).await;

        let mut second = Outgoing::default();
        publish(&mut inner, &mut second, id, node("speaker.a", "A", true));

        assert!(drain(second, &mut rx).await.is_empty());
    }

    #[tokio::test]
    async fn only_the_clusters_that_differ_are_reported() {
        let (mut inner, id, mut rx) = registered("speaker.a").await;

        let mut first = Outgoing::default();
        publish(&mut inner, &mut first, id, node("speaker.a", "A", true));
        drain(first, &mut rx).await;

        let mut second = Outgoing::default();
        publish(&mut inner, &mut second, id, node("speaker.a", "A", false));

        match drain(second, &mut rx).await.as_slice() {
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

    #[tokio::test]
    async fn a_renamed_node_is_reannounced_under_the_new_name() {
        // An attribute change has no field for the name or the entity id, so
        // a device renamed in Snapcast can only be reported by announcing it
        // again under the same node id. The entity id is a reservation in a
        // shared keyspace, so the move goes through the registry.
        let (mut inner, id, mut rx) = registered("speaker.a").await;

        let mut first = Outgoing::default();
        publish(&mut inner, &mut first, id, node("speaker.a", "A", true));
        drain(first, &mut rx).await;

        let mut second = Outgoing::default();
        publish(
            &mut inner,
            &mut second,
            id,
            node("speaker.kitchen", "Kitchen", true),
        );

        match drain(second, &mut rx).await.as_slice() {
            [FromIntegrationMessage::NodeAdded { node_id, node }] => {
                assert_eq!(*node_id, id);
                assert_eq!(node.entity_id, "speaker.kitchen");
                assert_eq!(node.name.as_deref(), Some("Kitchen"));
            }
            other => panic!("expected a re-announcement, got {other:?}"),
        }

        assert_eq!(
            inner.registrations[&id].entity_id(),
            "speaker.kitchen",
            "the registry should hold the new name, not just the announcement"
        );
    }

    /// A node admitted under a suffix keeps re-deriving the name it asked for,
    /// because that is what the Snapcast name slugs to. Reporting that as a
    /// rename every time would re-announce the node on every refresh forever.
    #[tokio::test]
    async fn a_node_admitted_under_a_suffix_settles_instead_of_reannouncing() {
        let (tx, mut rx) = mpsc::channel(16);
        let registry = IntegrationRegistry::for_test(INTEGRATION_NAME, tx);

        let _holder = registry.register(node("speaker.a", "Held", true)).await;
        let registration = registry
            .register(node("speaker.a", "A", true))
            .await
            .expect("the default policy admits it under a suffix");
        let id = registration.node_id();
        assert_eq!(registration.entity_id(), "speaker.a_2");

        let mut inner = Inner {
            registrations: HashMap::from([(id, registration)]),
            ..Default::default()
        };
        while rx.try_recv().is_ok() {}

        // Three refreshes all deriving the same requested name.
        let mut first = Outgoing::default();
        publish(&mut inner, &mut first, id, node("speaker.a", "A", true));
        assert_eq!(drain(first, &mut rx).await.len(), 1, "the first is new");

        for _ in 0..2 {
            let mut again = Outgoing::default();
            publish(&mut inner, &mut again, id, node("speaker.a", "A", true));
            assert!(
                drain(again, &mut rx).await.is_empty(),
                "a settled suffix is not a rename"
            );
        }

        assert_eq!(inner.registrations[&id].entity_id(), "speaker.a_2");
    }
}
