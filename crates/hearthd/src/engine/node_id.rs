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
//! value privately and the only way to obtain one is
//! [`NodeIdAllocator::allocate`], which the engine hands to each integration as
//! it registers it. The type is otherwise ordinary — `Copy`, comparable and
//! hashable — so passing ids around costs nothing.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use serde::Deserialize;
use serde::Serialize;

/// Locally assigned Matter node identifier.
///
/// Obtainable only from [`NodeIdAllocator::allocate`]. `Deserialize` is the
/// one exception and exists because the engine's state snapshot round trips
/// through serde; it is not a way for an integration to name a node it does
/// not own.
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
/// Cloning shares the counter rather than restarting it, so every integration
/// draws from one sequence.
#[derive(Debug, Clone)]
pub struct NodeIdAllocator {
    next: Arc<AtomicU64>,
}

impl NodeIdAllocator {
    /// Create the allocator. Engine-internal: having exactly one per engine is
    /// what makes the ids unique.
    pub(super) fn new() -> Self {
        Self {
            next: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Take the next unused identifier.
    pub fn allocate(&self) -> NodeId {
        NodeId(self.next.fetch_add(1, Ordering::Relaxed))
    }

    /// An allocator for tests and integrations that are constructed outside the
    /// engine's normal registration path.
    ///
    /// Test-only in spirit: outside tests, a second allocator would count from
    /// 1 again and hand out ids the real one has already given away, which is
    /// the collision this type exists to prevent. Exposed to integration
    /// constructors so they can be instantiated in registry functions without
    /// needing engine internals.
    pub fn for_test() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_of_the_allocator_share_one_sequence() {
        // Each integration holds its own handle. Without the sharing, two
        // integrations both counting from 1 hand the same id to different
        // devices.
        let engine = NodeIdAllocator::new();
        let first = engine.clone();
        let second = engine.clone();

        let ids = [
            first.allocate(),
            second.allocate(),
            first.allocate(),
            second.allocate(),
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
