//! Deployment schema describing what entities exist in the running
//! fabric, used by the type checker to validate structurally-bound
//! state paths like `state.lights.living_room_lamp`.
//!
//! The schema is synthesised from a live `engine::state::State` snapshot
//! by splitting each `Node::entity_id` on `.` into a (domain, slug)
//! pair. The domain becomes a top-level field on `state` (e.g.
//! `lights`, `binary_sensors`); the slug becomes a field on that
//! domain group, typed as a `Node` reference.
//!
//! A `None` schema means "no entity bindings available" — the checker
//! falls back to the static facet shape of `engine::state::State` for
//! field lookups, which only exposes the raw `nodes` / `by_entity_id`
//! maps.

use std::collections::BTreeMap;

use crate::engine::state::State;
use crate::matter::NodeId;

/// A per-deployment view of which entity IDs live in which domain.
#[derive(Debug, Clone, Default)]
pub struct DeploymentSchema {
    /// `domain → { slug → node_id }`. Ordered for deterministic
    /// snapshot output.
    pub domains: BTreeMap<String, BTreeMap<String, NodeId>>,
}

impl DeploymentSchema {
    /// Build a schema by splitting every node's `entity_id` on `.`.
    /// Nodes whose `entity_id` does not contain a `.` are skipped.
    pub fn from_state(state: &State) -> Self {
        let mut domains: BTreeMap<String, BTreeMap<String, NodeId>> = BTreeMap::new();
        for (node_id, node) in &state.nodes {
            let Some((domain, slug)) = node.entity_id.split_once('.') else {
                continue;
            };
            domains
                .entry(domain.to_string())
                .or_default()
                .insert(slug.to_string(), *node_id);
        }
        Self { domains }
    }

    /// Look up the NodeId for `<domain>.<slug>`, if registered.
    pub fn lookup(&self, domain: &str, slug: &str) -> Option<NodeId> {
        self.domains.get(domain)?.get(slug).copied()
    }

    /// Returns true if the named domain exists in this schema.
    pub fn has_domain(&self, domain: &str) -> bool {
        self.domains.contains_key(domain)
    }

    /// Returns true if `<domain>.<slug>` resolves to a known node.
    pub fn has_slug(&self, domain: &str, slug: &str) -> bool {
        self.domains
            .get(domain)
            .is_some_and(|slugs| slugs.contains_key(slug))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::matter::Endpoint;
    use crate::matter::Node;

    fn fake_node(id: NodeId, entity_id: &str) -> (NodeId, Node) {
        (
            id,
            Node {
                id,
                entity_id: entity_id.to_string(),
                integration: "test".to_string(),
                name: None,
                endpoints: HashMap::<u16, Endpoint>::new(),
            },
        )
    }

    #[test]
    fn from_state_groups_by_domain_prefix() {
        let mut state = State::default();
        state.nodes.insert(1, fake_node(1, "light.kitchen").1);
        state
            .nodes
            .insert(2, fake_node(2, "light.living_room_lamp").1);
        state
            .nodes
            .insert(3, fake_node(3, "binary_sensor.kitchen_motion").1);
        state.nodes.insert(4, fake_node(4, "no_dot_here").1);

        let schema = DeploymentSchema::from_state(&state);
        assert_eq!(schema.lookup("light", "kitchen"), Some(1));
        assert_eq!(schema.lookup("light", "living_room_lamp"), Some(2));
        assert_eq!(schema.lookup("binary_sensor", "kitchen_motion"), Some(3));
        assert!(schema.has_domain("light"));
        assert!(schema.has_slug("binary_sensor", "kitchen_motion"));
        assert!(!schema.has_slug("light", "missing"));
        // entity_ids without a `.` are skipped entirely.
        assert_eq!(schema.domains.len(), 2);
    }
}
