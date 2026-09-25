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
use std::collections::HashSet;
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
use crate::engine::Integration;
use crate::engine::IntegrationSender;
use crate::engine::ToIntegrationMessage;
use crate::matter::AttributeWrite;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;
use crate::matter::LocalKey;
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

    #[error("unknown endpoint {endpoint_id} on {key}")]
    UnknownEndpoint {
        key: LocalKey,
        endpoint_id: EndpointId,
    },

    #[error("no snapcast command mapping for {key} command {command:?}")]
    Unmapped {
        key: LocalKey,
        command: ClusterCommand,
    },

    #[error("snapcast does not accept attribute writes: {key} {write:?}")]
    UnsupportedWrite {
        key: LocalKey,
        write: AttributeWrite,
    },

    #[error("{method} failed")]
    Rpc {
        method: &'static str,
        #[source]
        source: RpcClientError,
    },
}

/// Mutable view of the server.
#[derive(Default)]
struct Inner {
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
    /// Last node published for each key, so a refresh can report what actually
    /// changed instead of re-announcing everything.
    published: HashMap<LocalKey, Node>,
}

/// What a refresh found to tell the engine, collected under the lock and
/// sent once it is released.
#[derive(Debug)]
enum Outgoing {
    Added(Node),
    Report {
        key: LocalKey,
        endpoint_id: EndpointId,
        cluster: Cluster,
    },
    Removed(LocalKey),
}

/// Everything built during `setup` and shared with the background tasks.
struct State {
    client: SnapcastRpcClient,
    to_engine: IntegrationSender,
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
                key,
                endpoint_id,
                command,
            } => {
                if endpoint_id != mapper::SNAPCAST_ENDPOINT {
                    return Err(CommandError::UnknownEndpoint { key, endpoint_id });
                }

                let unmapped = || CommandError::Unmapped {
                    key: key.clone(),
                    command: command.clone(),
                };
                let target = mapper::target(&key).ok_or_else(unmapped)?;

                let (method, params) = {
                    let inner = state.inner.lock().await;
                    let ctx = mapper::CommandContext {
                        groups: &inner.groups,
                        clients: &inner.clients,
                        stream_by_index: &inner.stream_by_index,
                    };
                    mapper::command_to_rpc(target, &command, &ctx).ok_or_else(unmapped)?
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
            ToIntegrationMessage::WriteAttribute { key, write, .. } => {
                return Err(CommandError::UnsupportedWrite { key, write });
            }
        }
        Ok(())
    }
}

/// Fetch the full server status and publish whatever it changed.
async fn refresh(state: &State) -> Result<(), RefreshError> {
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

        let mut live: HashSet<LocalKey> = HashSet::new();
        let mut groups = HashMap::new();
        let mut clients = HashMap::new();

        for group in &status.groups {
            groups.insert(group.id.clone(), group.clone());

            let node = mapper::group_node(group, &inner.streams, &inner.stream_indices);
            live.insert(node.key.clone());
            publish(&mut inner, &mut messages, node);

            for client in &group.clients {
                clients.insert(client.id.clone(), client.clone());

                let node = mapper::client_node(client);
                live.insert(node.key.clone());
                publish(&mut inner, &mut messages, node);
            }
        }

        let departed: Vec<LocalKey> = inner
            .published
            .keys()
            .filter(|key| !live.contains(*key))
            .cloned()
            .collect();
        for key in departed {
            inner.published.remove(&key);
            messages.push(Outgoing::Removed(key));
        }

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
        let sent = match message {
            Outgoing::Added(node) => state.to_engine.node_added(node).await,
            Outgoing::Report {
                key,
                endpoint_id,
                cluster,
            } => state.to_engine.report(&key, endpoint_id, cluster).await,
            Outgoing::Removed(key) => state.to_engine.node_removed(&key).await,
        };
        if sent.is_err() {
            return Err(RefreshError::EngineGone);
        }
    }

    Ok(())
}

/// Queue the messages that move a node from its published form to `node`.
///
/// A node the engine has not seen is announced whole; one it already has
/// reports only the clusters whose contents differ, which is what makes the
/// engine emit attribute-change events rather than repeated discovery.
fn publish(inner: &mut Inner, messages: &mut Vec<Outgoing>, node: Node) {
    match inner.published.get(&node.key) {
        // The name is only carried by NodeAdded, so a device renamed in
        // Snapcast has to be re-announced rather than described by a cluster
        // diff that has no field for it.
        Some(previous) if previous.name != node.name => {
            messages.push(Outgoing::Added(node.clone()));
        }
        None => messages.push(Outgoing::Added(node.clone())),
        Some(previous) => {
            for (endpoint_id, endpoint) in &node.endpoints {
                for (name, cluster) in &endpoint.clusters {
                    let unchanged = previous
                        .endpoints
                        .get(endpoint_id)
                        .and_then(|e| e.clusters.get(name))
                        .is_some_and(|p| p == cluster);
                    if !unchanged {
                        messages.push(Outgoing::Report {
                            key: node.key.clone(),
                            endpoint_id: *endpoint_id,
                            cluster: cluster.clone(),
                        });
                    }
                }
            }
        }
    }

    inner.published.insert(node.key.clone(), node);
}

#[async_trait]
impl Integration for SnapcastIntegration {
    fn name(&self) -> &str {
        INTEGRATION_NAME
    }

    async fn setup(&mut self, tx: IntegrationSender) -> Result<(), Box<dyn Error + Send>> {
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
    use crate::matter::Endpoint;
    use crate::matter::OnOffCluster;

    fn node(name: &str, on_off: bool) -> Node {
        let mut endpoint = Endpoint::default();
        endpoint.clusters.insert(
            crate::matter::CLUSTER_NAME_ON_OFF.to_string(),
            Cluster::OnOff(OnOffCluster { on_off }),
        );
        let mut endpoints = HashMap::new();
        endpoints.insert(mapper::SNAPCAST_ENDPOINT, endpoint);
        Node {
            key: LocalKey::from("client/a"),
            name: Some(name.to_string()),
            endpoints,
        }
    }

    #[test]
    fn a_node_the_engine_has_not_seen_is_announced_whole() {
        let mut inner = Inner::default();
        let mut messages = Vec::new();

        publish(&mut inner, &mut messages, node("A", true));

        assert!(matches!(
            messages.as_slice(),
            [Outgoing::Added(node)] if node.key == LocalKey::from("client/a")
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

        publish(&mut inner, &mut messages, node("A", true));
        messages.clear();
        publish(&mut inner, &mut messages, node("A", true));

        assert!(messages.is_empty());
    }

    #[test]
    fn only_the_clusters_that_differ_are_reported() {
        let mut inner = Inner::default();
        let mut messages = Vec::new();

        publish(&mut inner, &mut messages, node("A", true));
        messages.clear();
        publish(&mut inner, &mut messages, node("A", false));

        match messages.as_slice() {
            [
                Outgoing::Report {
                    key,
                    endpoint_id,
                    cluster: Cluster::OnOff(c),
                },
            ] => {
                assert_eq!(*key, LocalKey::from("client/a"));
                assert_eq!(*endpoint_id, mapper::SNAPCAST_ENDPOINT);
                assert!(!c.on_off);
            }
            other => panic!("expected one OnOff attribute change, got {other:?}"),
        }
    }

    #[test]
    fn a_renamed_node_is_reannounced() {
        // An attribute change has no field for the name, so
        // a device renamed in Snapcast can only be reported by announcing it
        // again under the same key.
        let mut inner = Inner::default();
        let mut messages = Vec::new();

        publish(&mut inner, &mut messages, node("A", true));
        messages.clear();
        publish(&mut inner, &mut messages, node("Kitchen", true));

        match messages.as_slice() {
            [Outgoing::Added(node)] => {
                assert_eq!(node.key, LocalKey::from("client/a"));
                assert_eq!(node.name.as_deref(), Some("Kitchen"));
            }
            other => panic!("expected a re-announcement, got {other:?}"),
        }
    }
}
