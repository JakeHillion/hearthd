//! The one message type on the engine's stream.
//!
//! Every producer feeds the same bounded queue and the engine loop consumes
//! it in arrival order. Reports carry the Matter data model defined in
//! `crate::matter`; integrations translate their native representation at
//! their boundary.

use crate::engine::NodeId;
use crate::matter::Cluster;
use crate::matter::EndpointId;
use crate::matter::Node;

#[derive(Debug, Clone)]
pub enum Event {
    /// A node was discovered and is now known to the integration.
    /// The full `Node` is included so the engine can populate its state
    /// snapshot atomically.
    NodeAdded { node_id: NodeId, node: Node },

    /// A node was removed (device unpaired, integration lost track, etc.)
    NodeRemoved { node_id: NodeId },

    /// A cluster snapshot from the owning integration. The only thing that
    /// changes state. The full new cluster is sent (Matter would send
    /// per-attribute reports, but a cluster-level snapshot is simpler and
    /// lossless for the clusters we model).
    Report {
        node_id: NodeId,
        endpoint_id: EndpointId,
        cluster: Cluster,
    },
}
