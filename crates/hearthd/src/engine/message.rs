//! Type-safe message system for hearthd
//!
//! Messages are split by direction to enforce correct usage at compile time:
//! - `FromIntegrationMessage`: Events from integrations to the engine
//! - `ToIntegrationMessage`: Commands from the engine to integrations
//!
//! Both directions speak the Matter data model defined in `crate::matter`.
//! Integrations translate their native representation at their boundary.

use crate::engine::NodeId;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::ClusterWrite;
use crate::matter::EndpointId;
use crate::matter::Node;

/// Messages FROM integrations TO the engine (events/state updates)
#[derive(Debug, Clone)]
pub enum FromIntegrationMessage {
    /// A node was discovered and is now known to the integration.
    /// The full `Node` is included so the engine can populate its state
    /// snapshot atomically.
    NodeAdded { node_id: NodeId, node: Node },

    /// A node was removed (device unpaired, integration lost track, etc.)
    NodeRemoved { node_id: NodeId },

    /// A cluster's attributes changed. The full new cluster snapshot is
    /// sent (Matter would send per-attribute reports, but a cluster-level
    /// snapshot is simpler and lossless for the clusters we model).
    AttributeChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        cluster: Cluster,
    },
}

/// Messages FROM the engine TO integrations (commands or attribute writes)
#[derive(Debug, Clone)]
pub enum ToIntegrationMessage {
    /// Invoke a real Matter cluster command on the given endpoint.
    InvokeCommand {
        node_id: NodeId,
        endpoint_id: EndpointId,
        command: ClusterCommand,
    },

    /// Write one or more cluster attributes on the given endpoint.
    ///
    /// This is the Matter `WriteAttribute` interaction: each `ClusterWrite`
    /// targets a single cluster and carries only the attributes to change.
    WriteAttributes {
        node_id: NodeId,
        endpoint_id: EndpointId,
        writes: Vec<ClusterWrite>,
    },
}
