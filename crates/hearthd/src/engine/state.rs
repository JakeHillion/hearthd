use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use crate::matter::Node;
use crate::matter::NodeId;

/// Centralized snapshot of the entire engine state.
///
/// All known devices are represented as Matter nodes keyed by NodeId. The
/// `by_entity_id` reverse index lets the API resolve user-facing entity_id
/// strings (e.g. "light.living_room") back to a NodeId.
#[derive(Debug, Clone, Default, Serialize, Deserialize, facet::Facet)]
pub struct State {
    pub nodes: HashMap<NodeId, Node>,
    pub by_entity_id: HashMap<String, NodeId>,
}
