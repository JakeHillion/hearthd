use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use crate::engine::NodeId;
use crate::matter::Node;

/// Centralized snapshot of the entire engine state.
///
/// All known devices are represented as Matter nodes keyed by NodeId. The
/// two name tables are how anything outside the engine addresses a node;
/// see [`Engine::resolve`](super::Engine::resolve).
#[derive(Debug, Clone, Default, Serialize, Deserialize, facet::Facet)]
pub struct State {
    pub nodes: HashMap<NodeId, Node>,

    /// Names given in configuration. Fixed for the process, and each one
    /// resolves whether or not its node has been announced yet.
    pub aliases: HashMap<String, NodeId>,

    /// The slugged names integrations gave their nodes. A name carried by
    /// more than one node is ambiguous and resolves to none of them.
    pub names: HashMap<String, Vec<NodeId>>,
}
