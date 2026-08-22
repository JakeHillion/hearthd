//! Snapcast integration for hearthd.
//!
//! Controls a Snapserver over its raw TCP JSON-RPC control protocol (port
//! 1705). Exposes each group as a media-player node and each client as a
//! speaker node.

use std::collections::HashMap;
use std::error::Error;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::client::SnapcastRpcClient;
use super::config::Config;
use super::mapper;
use super::models::Client;
use super::models::Group;
use super::models::ServerStatus;
use super::models::Stream;
use crate::engine::FromIntegrationMessage;
use crate::engine::FromIntegrationSender;
use crate::engine::Integration;
use crate::engine::NodeId;
use crate::engine::NodeIdAllocator;
use crate::engine::ToIntegrationMessage;

/// Integration name reported to the engine.
const INTEGRATION_NAME: &str = "snapcast";

/// Internal state owned by the integration.
struct Inner {
    /// Allocator used to mint stable NodeIds for groups and clients.
    node_ids: NodeIdAllocator,

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
    /// Reverse lookup from (stream_index, group_id) to stream_id.
    index_to_stream: HashMap<(u8, String), String>,
    /// Current group state.
    groups: HashMap<String, Group>,
    /// Current client state.
    clients: HashMap<String, Client>,
}

type SharedInner = std::sync::Arc<Mutex<Inner>>;

impl Inner {
    fn new(node_ids: NodeIdAllocator) -> Self {
        Self {
            node_ids,
            group_node_ids: HashMap::new(),
            client_node_ids: HashMap::new(),
            node_to_group: HashMap::new(),
            node_to_client: HashMap::new(),
            streams: HashMap::new(),
            stream_indices: HashMap::new(),
            index_to_stream: HashMap::new(),
            groups: HashMap::new(),
            clients: HashMap::new(),
        }
    }
}

/// Snapcast integration.
pub struct SnapcastIntegration {
    config: Config,
    inner: SharedInner,
    to_engine: Option<FromIntegrationSender>,
    rpc_task: Option<tokio::task::JoinHandle<()>>,
}

impl SnapcastIntegration {
    /// Create a new integration from configuration.
    pub fn new(config: Config, node_ids: NodeIdAllocator) -> Self {
        Self {
            config,
            inner: std::sync::Arc::new(Mutex::new(Inner::new(node_ids))),
            to_engine: None,
            rpc_task: None,
        }
    }

    /// Bootstrap nodes from a full server status.
    async fn apply_full_status(
        &self,
        client: &SnapcastRpcClient,
    ) -> Result<(), Box<dyn Error + Send>> {
        let status: ServerStatus = client
            .request("Server.GetStatus", serde_json::Value::Null)
            .await
            .map_err(|e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::other(e.to_string()))
            })?;

        let to_engine = self
            .to_engine
            .as_ref()
            .ok_or_else(|| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "No engine sender",
                ))
            })?;

        let mut inner = self.inner.lock().await;

        // Update stream set and indices.
        inner.streams.clear();
        inner.stream_indices.clear();
        inner.index_to_stream.clear();
        for (idx, stream) in status.streams.iter().enumerate() {
            let index = idx as u8;
            inner.streams.insert(stream.id.clone(), stream.clone());
            inner.stream_indices.insert(stream.id.clone(), index);
            // index_to_stream is keyed by (index, group_id) but group ids are
            // not known yet; fill in per group below.
        }

        // Update groups and announce new/changed nodes.
        let mut new_groups: HashMap<String, NodeId> = HashMap::new();
        let stream_indices_snapshot = inner.stream_indices.clone();
        for group in &status.groups {
            let entity_id = mapper::group_entity_id(&group.id);
            let fresh_id = inner.node_ids.allocate();
            let node_id = *inner
                .group_node_ids
                .entry(group.id.clone())
                .or_insert(fresh_id);
            new_groups.insert(group.id.clone(), node_id);

            // Build (index, group_id) -> stream_id reverse lookup.
            for (stream_id, index) in &stream_indices_snapshot {
                inner
                    .index_to_stream
                    .insert((*index, group.id.clone()), stream_id.clone());
            }

            let node = mapper::group_node(group, &inner.streams, &inner.stream_indices, &entity_id);
            if let Err(e) = to_engine
                .send(FromIntegrationMessage::NodeAdded { node_id, node })
                .await
            {
                warn!("Failed to send NodeAdded for group {}: {e}", group.id);
            }
            inner.groups.insert(group.id.clone(), group.clone());
        }

        // Remove groups that disappeared.
        let removed_groups: Vec<String> = inner
            .group_node_ids
            .keys()
            .filter(|k| !new_groups.contains_key(*k))
            .cloned()
            .collect();
        for group_id in removed_groups {
            if let Some(node_id) = inner.group_node_ids.remove(&group_id) {
                inner.node_to_group.remove(&node_id);
                if let Err(e) = to_engine
                    .send(FromIntegrationMessage::NodeRemoved { node_id })
                    .await
                {
                    warn!("Failed to send NodeRemoved for group {group_id}: {e}");
                }
            }
        }

        inner.group_node_ids = new_groups;
        let group_node_ids_snapshot = inner.group_node_ids.clone();
        for (group_id, node_id) in &group_node_ids_snapshot {
            inner.node_to_group.insert(*node_id, group_id.clone());
        }

        // Update clients and announce new/changed nodes.
        let mut new_clients: HashMap<String, NodeId> = HashMap::new();
        for group in &status.groups {
            for client in &group.clients {
                let entity_id = mapper::client_entity_id(&client.id);
                let fresh_id = inner.node_ids.allocate();
                let node_id = *inner
                    .client_node_ids
                    .entry(client.id.clone())
                    .or_insert(fresh_id);
                new_clients.insert(client.id.clone(), node_id);

                let node = mapper::client_node(client, &entity_id);
                if let Err(e) = to_engine
                    .send(FromIntegrationMessage::NodeAdded { node_id, node })
                    .await
                {
                    warn!("Failed to send NodeAdded for client {}: {e}", client.id);
                }
                inner.clients.insert(client.id.clone(), client.clone());
            }
        }

        // Remove clients that disappeared.
        let removed_clients: Vec<String> = inner
            .client_node_ids
            .keys()
            .filter(|k| !new_clients.contains_key(*k))
            .cloned()
            .collect();
        for client_id in removed_clients {
            if let Some(node_id) = inner.client_node_ids.remove(&client_id) {
                inner.node_to_client.remove(&node_id);
                if let Err(e) = to_engine
                    .send(FromIntegrationMessage::NodeRemoved { node_id })
                    .await
                {
                    warn!("Failed to send NodeRemoved for client {client_id}: {e}");
                }
            }
        }

        inner.client_node_ids = new_clients;
        let client_node_ids_snapshot = inner.client_node_ids.clone();
        for (client_id, node_id) in &client_node_ids_snapshot {
            inner.node_to_client.insert(*node_id, client_id.clone());
        }

        info!(
            "Snapcast status applied: {} groups, {} clients, {} streams",
            inner.groups.len(),
            inner.clients.len(),
            inner.streams.len()
        );
        Ok(())
    }
}

#[async_trait]
impl Integration for SnapcastIntegration {
    fn name(&self) -> &str {
        INTEGRATION_NAME
    }

    async fn setup(
        &mut self,
        tx: FromIntegrationSender,
        _node_ids: NodeIdAllocator,
    ) -> Result<(), Box<dyn Error + Send>> {
        self.to_engine = Some(tx.clone());

        let (client, mut notification_rx) = SnapcastRpcClient::new(
            self.config.host.clone(),
            self.config.port,
            self.config.reconnect_interval_ms,
        );

        // Bootstrap and start background reader.
        self.apply_full_status(&client).await?;
        let rpc_task = client.spawn();
        self.rpc_task = Some(rpc_task);

        let weak_inner = std::sync::Arc::downgrade(&self.inner);
        let _to_engine = tx.clone();
        tokio::spawn(async move {
            while let Some(notification) = notification_rx.recv().await {
                match notification.method.as_str() {
                    "Server.OnUpdate"
                    | "Group.OnMute"
                    | "Group.OnStreamChanged"
                    | "Group.OnNameChanged"
                    | "Client.OnConnect"
                    | "Client.OnDisconnect"
                    | "Client.OnVolumeChanged"
                    | "Client.OnNameChanged"
                    | "Stream.OnUpdate"
                    | "Stream.OnProperties" => {
                        // Trigger a full refresh. Reconnecting and creating a
                        // fresh client each time is expensive; in production we
                        // keep a reference to the client in Inner. For the
                        // initial version, this loop signals the integration
                        // task to refresh by scheduling a refresh.
                        if let Some(inner) = weak_inner.upgrade() {
                            // We cannot call `apply_full_status` here because
                            // we don't own `client`. Instead we queue a refresh
                            // signal. This is a placeholder for a real
                            // refresh channel.
                            let _ = inner;
                        }
                    }
                    _ => {
                        debug!("Ignoring Snapcast notification: {}", notification.method);
                    }
                }
            }
        });

        Ok(())
    }

    async fn handle_message(
        &mut self,
        msg: ToIntegrationMessage,
    ) -> Result<(), Box<dyn Error + Send>> {
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
                    let inner = self.inner.lock().await;
                    mapper::command_to_rpc(
                        node_id,
                        &command,
                        &inner.node_to_group,
                        &inner.node_to_client,
                        &inner.index_to_stream,
                    )
                    .ok_or_else(|| -> Box<dyn Error + Send> {
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!(
                                "No Snapcast command mapping for node {node_id} command {command:?}"
                            ),
                        ))
                    })?
                };

                // TODO: send via the RPC client. The current structure stores
                // the client in a spawned task; we need to expose a request
                // sender. For now we log the intended command.
                info!("Would send Snapcast RPC {method} with {params}");

                // Re-fetch status after a short delay to pick up the change.
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
        if let Some(task) = self.rpc_task.take() {
            task.abort();
        }
        Ok(())
    }
}
