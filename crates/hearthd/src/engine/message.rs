//! Messages from the engine to integrations.
//!
//! The other direction is [`Event`](super::Event): integrations put events on
//! the engine's stream rather than sending the engine a message of their
//! own. Both speak the Matter data model defined in `crate::matter`.

use crate::matter::AttributeWrite;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;
use crate::matter::LocalKey;

/// Messages FROM the engine TO integrations (commands)
#[derive(Debug, Clone)]
pub enum ToIntegrationMessage {
    /// Invoke a Matter cluster command on the given endpoint.
    InvokeCommand {
        /// The integration's own name for the node, as it announced it.
        key: LocalKey,
        endpoint_id: EndpointId,
        command: ClusterCommand,
    },

    /// Write a Matter cluster attribute on the given endpoint.
    WriteAttribute {
        /// The integration's own name for the node, as it announced it.
        key: LocalKey,
        endpoint_id: EndpointId,
        write: AttributeWrite,
    },
}
