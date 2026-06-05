//! Deployment schema: which entity slugs the running fabric actually has.
//!
//! This is the deployment half of an `entity_id`. The domain half is a
//! [`Domain`], which every install knows; which slugs exist in a domain is
//! particular to one house and changes as devices are discovered.
//!
//! The checker never sees this. It types `state.light.living_room_lamp`
//! structurally and records the name as a symbol; a [`DeploymentSchema`] is
//! what [`crate::automations::relocate`] resolves those symbols against,
//! long after the automation has been compiled. A device that turns up late
//! — Zigbee2MQTT discovery is not instant — changes the schema and calls for
//! a relink, not a recompile.

use std::collections::BTreeMap;

use enum_map::EnumMap;

use crate::automations::domain::Domain;
use crate::engine::NodeId;
use crate::engine::state::State;

/// A per-deployment view of which slugs live in which domain.
#[derive(Debug, Clone, Default)]
pub struct DeploymentSchema {
    /// `domain → { slug → node id }`.
    ///
    /// Total over [`Domain`] by construction: a domain this deployment has
    /// nothing in is an empty map, not a missing key. That is the same
    /// promise the checker makes when it types `state.<domain>` on every
    /// deployment, so anything that projects `state` from this schema gets
    /// every domain without having to remember to add the empty ones.
    ///
    /// The inner map is ordered rather than hashed so anything derived from
    /// it comes out the same way on every run.
    pub domains: EnumMap<Domain, BTreeMap<String, NodeId>>,
}

impl DeploymentSchema {
    /// Build a schema by splitting every node's `entity_id` on its first `.`.
    ///
    /// A node whose prefix is not a known [`Domain`] is skipped. Nothing can
    /// name it — an automation reaches an entity through its domain, and a
    /// domain the language has no variant for cannot be written — so there
    /// is no symbol it could ever resolve. An integration producing one is
    /// the bug, and this is not the place that reports it.
    pub fn from_state(state: &State) -> Self {
        let mut domains: EnumMap<Domain, BTreeMap<String, NodeId>> = EnumMap::default();
        for (node_id, node) in &state.nodes {
            let Some((prefix, slug)) = node.entity_id.split_once('.') else {
                continue;
            };
            let Some(domain) = Domain::parse(prefix) else {
                continue;
            };
            domains[domain].insert(slug.to_string(), *node_id);
        }
        Self { domains }
    }

    /// The node `<domain>.<slug>` names, if the deployment has one.
    pub fn lookup(&self, domain: Domain, slug: &str) -> Option<NodeId> {
        self.domains[domain].get(slug).copied()
    }
}
