//! Type-safe message system for hearthd
//!
//! Messages are split by direction to enforce correct usage at compile time:
//! - `FromIntegrationMessage`: Events from integrations to the engine
//! - `ToIntegrationMessage`: Commands from the engine to integrations
//!
//! Both directions speak the Matter data model defined in `crate::matter`.
//! Integrations translate their native representation at their boundary.

use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;
use crate::matter::Node;
use crate::matter::NodeId;

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

/// Messages FROM the engine TO integrations (commands)
#[derive(Debug, Clone)]
pub enum ToIntegrationMessage {
    /// Invoke a Matter cluster command on the given endpoint.
    InvokeCommand {
        node_id: NodeId,
        endpoint_id: EndpointId,
        command: ClusterCommand,
    },
}
