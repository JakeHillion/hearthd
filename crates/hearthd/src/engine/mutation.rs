//! Engine-side mutation validation.
//!
//! Before a command or attribute write is forwarded to the integration that
//! owns a node, the engine checks that the target exists and that the
//! operation is structurally allowed. Device-specific rejection (e.g. an
//! unsupported mode value) still happens inside the integration.

use std::collections::HashSet;

use crate::engine::NodeId;
use crate::matter::ClusterCommand;
use crate::matter::ClusterWrite;
use crate::matter::EndpointId;

/// Why a mutation cannot be applied.
#[derive(Debug, Clone, PartialEq)]
pub enum MutationError {
    /// No node with this id is known.
    UnknownNode { node_id: NodeId },
    /// The node exists but does not expose this endpoint.
    UnknownEndpoint {
        node_id: NodeId,
        endpoint_id: EndpointId,
    },
    /// The endpoint does not carry the cluster being targeted.
    UnsupportedCluster {
        node_id: NodeId,
        endpoint_id: EndpointId,
        cluster_name: String,
    },
    /// The cluster is present but is not writable.
    ReadOnlyCluster {
        node_id: NodeId,
        endpoint_id: EndpointId,
        cluster_name: String,
    },
    /// The write request carried no actual fields to change.
    EmptyWrite,
}

impl std::fmt::Display for MutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MutationError::UnknownNode { node_id } => write!(f, "unknown node {node_id}"),
            MutationError::UnknownEndpoint {
                node_id,
                endpoint_id,
            } => write!(f, "node {node_id} has no endpoint {endpoint_id}"),
            MutationError::UnsupportedCluster {
                node_id,
                endpoint_id,
                cluster_name,
            } => write!(
                f,
                "node {node_id} endpoint {endpoint_id} does not support cluster {cluster_name}"
            ),
            MutationError::ReadOnlyCluster {
                node_id,
                endpoint_id,
                cluster_name,
            } => write!(
                f,
                "cluster {cluster_name} on node {node_id} endpoint {endpoint_id} is read-only"
            ),
            MutationError::EmptyWrite => write!(f, "attribute write carried no fields to set"),
        }
    }
}

impl std::error::Error for MutationError {}

/// Set of cluster names that support attribute writes in this build.
const WRITABLE_CLUSTER_NAMES: [&str; 1] = [crate::matter::CLUSTER_NAME_THERMOSTAT];

/// Validate an attribute write against the current state snapshot.
///
/// Checks that the node and endpoint exist, that every targeted cluster is
/// present on the endpoint, and that at least one field is actually being set.
pub fn validate_write(
    state: &crate::engine::state::State,
    node_id: NodeId,
    endpoint_id: EndpointId,
    writes: &[ClusterWrite],
) -> Result<(), MutationError> {
    if writes.iter().all(|w| !w.has_any_field()) {
        return Err(MutationError::EmptyWrite);
    }

    let node = state
        .nodes
        .get(&node_id)
        .ok_or(MutationError::UnknownNode { node_id })?;

    let endpoint = node
        .endpoints
        .get(&endpoint_id)
        .ok_or(MutationError::UnknownEndpoint {
            node_id,
            endpoint_id,
        })?;

    let writable: HashSet<&str> = WRITABLE_CLUSTER_NAMES.iter().copied().collect();

    for write in writes {
        let cluster_name = write.cluster_name();
        if !endpoint.clusters.contains_key(cluster_name) {
            return Err(MutationError::UnsupportedCluster {
                node_id,
                endpoint_id,
                cluster_name: cluster_name.to_string(),
            });
        }
        if !writable.contains(cluster_name) {
            return Err(MutationError::ReadOnlyCluster {
                node_id,
                endpoint_id,
                cluster_name: cluster_name.to_string(),
            });
        }
    }

    Ok(())
}

/// Validate a command invocation against the current state snapshot.
///
/// Checks that the node and endpoint exist and that the targeted cluster is
/// present on the endpoint. The integration is still responsible for deciding
/// whether the specific command is supported.
pub fn validate_command(
    state: &crate::engine::state::State,
    node_id: NodeId,
    endpoint_id: EndpointId,
    command: &ClusterCommand,
) -> Result<(), MutationError> {
    let node = state
        .nodes
        .get(&node_id)
        .ok_or(MutationError::UnknownNode { node_id })?;

    let endpoint = node
        .endpoints
        .get(&endpoint_id)
        .ok_or(MutationError::UnknownEndpoint {
            node_id,
            endpoint_id,
        })?;

    let cluster_name = match command {
        ClusterCommand::OnOff(_) => crate::matter::CLUSTER_NAME_ON_OFF,
        ClusterCommand::LevelControl(_) => crate::matter::CLUSTER_NAME_LEVEL_CONTROL,
        ClusterCommand::Thermostat(_) => crate::matter::CLUSTER_NAME_THERMOSTAT,
        ClusterCommand::FanControl(_) => crate::matter::CLUSTER_NAME_FAN_CONTROL,
        ClusterCommand::DehumidificationControl(_) => {
            crate::matter::CLUSTER_NAME_DEHUMIDIFICATION_CONTROL
        }
        ClusterCommand::ThermostatUserInterfaceConfiguration(_) => {
            crate::matter::CLUSTER_NAME_THERMOSTAT_USER_INTERFACE_CONFIGURATION
        }
        ClusterCommand::ModeSelect(_) => crate::matter::CLUSTER_NAME_MODE_SELECT,
    };

    if !endpoint.clusters.contains_key(cluster_name) {
        return Err(MutationError::UnsupportedCluster {
            node_id,
            endpoint_id,
            cluster_name: cluster_name.to_string(),
        });
    }

    Ok(())
}
