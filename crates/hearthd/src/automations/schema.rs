//! Deployment schema: which entities exist in the running fabric.
//!
//! The checker types `state` from the facet shape of
//! [`crate::engine::state::State`], which exposes only the raw `nodes` and
//! `by_entity_id` maps. That is enough to type a lookup but not to bind one:
//! an automation that wants the living-room lamp has to name it, and no
//! static type knows which lamps a deployment has.
//!
//! A [`DeploymentSchema`] is that missing half, synthesised from a live
//! `State` snapshot by splitting each `Node::entity_id` on its first `.` into
//! a domain and a slug. The domain becomes a field on `state` (`light`,
//! `binary_sensor`, …) and the slug a field on that domain group, typed as a
//! `Node`. With one installed, `state = { light = { living_room_lamp }, ... }`
//! destructures; without one the checker falls back to the facet shape and
//! `light` is not a field at all.
//!
//! Being per-deployment is the point: a name that does not resolve is a
//! reported error rather than a lookup that fails at runtime, so a mistyped
//! entity is caught when an automation is compiled.

use std::collections::BTreeMap;

use crate::engine::NodeId;
use crate::engine::state::State;

/// A per-deployment view of which entity ids live in which domain.
#[derive(Debug, Clone, Default)]
pub struct DeploymentSchema {
    /// `domain → { slug → node id }`.
    ///
    /// Ordered rather than hashed so the synthetic fields the checker
    /// derives from it, and anything rendered from those, come out the same
    /// way on every run.
    pub domains: BTreeMap<String, BTreeMap<String, NodeId>>,
}

impl DeploymentSchema {
    /// Build a schema by splitting every node's `entity_id` on its first `.`.
    ///
    /// A node whose `entity_id` carries no `.` is skipped rather than given a
    /// domain of its own: the domain is what an automation destructures, and
    /// a bare name has none to offer.
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

    /// The node `<domain>.<slug>` names, if the deployment has one.
    pub fn lookup(&self, domain: &str, slug: &str) -> Option<NodeId> {
        self.domains.get(domain)?.get(slug).copied()
    }

    /// Whether the deployment has any node in `domain`.
    pub fn has_domain(&self, domain: &str) -> bool {
        self.domains.contains_key(domain)
    }

    /// Whether `<domain>.<slug>` names a node.
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
    use crate::matter::Node;

    fn fake_node(id: NodeId, entity_id: &str) -> Node {
        Node {
            id,
            entity_id: entity_id.to_string(),
            integration: "test".to_string(),
            name: None,
            endpoints: HashMap::new(),
        }
    }

    fn state_with(entries: &[(u64, &str)]) -> State {
        let mut state = State::default();
        for (raw, entity_id) in entries {
            let id = NodeId::from_raw(*raw);
            state.nodes.insert(id, fake_node(id, entity_id));
        }
        state
    }

    #[test]
    fn from_state_groups_by_domain_prefix() {
        let state = state_with(&[
            (1, "light.kitchen"),
            (2, "light.living_room_lamp"),
            (3, "binary_sensor.kitchen_motion"),
        ]);

        let schema = DeploymentSchema::from_state(&state);
        assert_eq!(schema.lookup("light", "kitchen"), Some(NodeId::from_raw(1)));
        assert_eq!(
            schema.lookup("light", "living_room_lamp"),
            Some(NodeId::from_raw(2))
        );
        assert_eq!(
            schema.lookup("binary_sensor", "kitchen_motion"),
            Some(NodeId::from_raw(3))
        );
        assert!(schema.has_domain("light"));
        assert!(schema.has_slug("binary_sensor", "kitchen_motion"));
        assert!(!schema.has_slug("light", "missing"));
    }

    /// A slug is only ever a slug of its own domain, so the same name under
    /// two domains is two nodes rather than one overwriting the other.
    #[test]
    fn from_state_keeps_domains_apart() {
        let state = state_with(&[(1, "light.kitchen"), (2, "binary_sensor.kitchen")]);

        let schema = DeploymentSchema::from_state(&state);
        assert_eq!(schema.lookup("light", "kitchen"), Some(NodeId::from_raw(1)));
        assert_eq!(
            schema.lookup("binary_sensor", "kitchen"),
            Some(NodeId::from_raw(2))
        );
    }

    /// An `entity_id` with no domain has nothing to destructure, so it is
    /// left out rather than becoming a domain of its own.
    #[test]
    fn from_state_skips_entity_ids_without_a_domain() {
        let state = state_with(&[(1, "light.kitchen"), (2, "no_dot_here")]);

        let schema = DeploymentSchema::from_state(&state);
        assert!(!schema.has_domain("no_dot_here"));
        assert_eq!(schema.domains.len(), 1);
    }
}
