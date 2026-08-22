//! Node identifiers, and the engine's allocator for them.
//!
//! Node ids live in one keyspace shared by every integration, and the engine
//! keys its state map and its command routing table by them. Uniqueness across
//! integrations is therefore a correctness property, not a convention: two
//! integrations that pick the same id produce a single node whose endpoints
//! come from whichever declaration arrived last, and whose commands are
//! delivered to whichever integration registered last.
//!
//! So an id is not an integer an integration can choose. [`NodeId`] holds its
//! value privately, and the allocator that mints one is reachable only from
//! [`NodeRegistry`](super::registry::NodeRegistry), which hands ids out as
//! part of registering a node. The type is otherwise ordinary — `Copy`,
//! comparable and hashable — so passing ids around costs nothing.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use serde::Deserialize;
use serde::Serialize;

/// Locally assigned Matter node identifier.
///
/// Obtainable only by registering a node. `Deserialize` is the one exception
/// and exists because the engine's state snapshot round trips through serde;
/// it is not a way for an integration to name a node it does not own.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, facet::Facet,
)]
#[serde(transparent)]
pub struct NodeId(u64);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
impl NodeId {
    /// Mint an identifier directly, for tests that need a node without an
    /// engine to allocate one.
    pub(crate) fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// Hands out node ids that are unique across every integration.
///
/// Engine-internal, and owned by the registry rather than exposed to
/// integrations: an id is only meaningful alongside the name and ownership
/// recorded with it, so the two are allocated together or not at all.
#[derive(Debug)]
pub(super) struct NodeIdAllocator {
    next: Arc<AtomicU64>,
}

impl NodeIdAllocator {
    pub(super) fn new() -> Self {
        Self {
            next: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Take the next unused identifier.
    pub(super) fn allocate(&self) -> NodeId {
        NodeId(self.next.fetch_add(1, Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_never_repeats() {
        let allocator = NodeIdAllocator::new();

        let ids = [
            allocator.allocate(),
            allocator.allocate(),
            allocator.allocate(),
            allocator.allocate(),
        ];

        let mut unique = ids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn allocation_starts_at_one() {
        assert_eq!(NodeIdAllocator::new().allocate(), NodeId::from_raw(1));
    }
}
