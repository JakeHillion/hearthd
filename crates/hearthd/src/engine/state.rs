use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use crate::automations::domain::Domain;
use crate::automations::entity_index::EntityIndex;
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

/// Relocation resolves entity names against the same index the API uses.
///
/// `by_entity_id` is already the reverse lookup from a user-facing name to a
/// node, maintained as integrations announce and rename devices, so a linked
/// automation and an API call agree on what a name means by construction
/// rather than by two indexes being kept in step.
impl EntityIndex for State {
    fn resolve(&self, domain: Domain, slug: &str) -> Option<NodeId> {
        self.by_entity_id
            .get(&format!("{}.{}", domain, slug))
            .copied()
    }
}
