//! Messages from the engine to integrations.
//!
//! The other direction is [`Event`](super::Event): integrations put events on
//! the engine's stream rather than sending the engine a message of their
//! own. Both speak the Matter data model defined in `crate::matter`.

use crate::engine::NodeId;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;

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
