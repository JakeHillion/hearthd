//! Node identifiers.
//!
//! Node ids live in one keyspace shared by every integration, and the engine
//! keys its state map and its bindings by them. An id is derived from the
//! owning integration's name and that integration's own stable key for the
//! device, so the same device gets the same id every run with nothing on
//! disk, and two integrations can never name the same node.

use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

use crate::matter::LocalKey;

/// What a node id is bound to: the integration that announced it and that
/// integration's own name for the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeKey {
    pub integration: Arc<str>,
    pub local: LocalKey,
}

impl std::fmt::Display for NodeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.integration, self.local)
    }
}

/// Matter node identifier, derived from the node's [`NodeKey`].
///
/// Written as 32 lowercase hex digits wherever it is shown or serialised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, facet::Facet)]
pub struct NodeId(u128);

impl NodeId {
    /// The id for the node `integration` calls `key`.
    ///
    /// XXH3-128 over the length-prefixed name and the key. The algorithm is
    /// fixed by its specification rather than by a Rust release, which is
    /// what lets the id be the same across restarts and upgrades. The length
    /// prefix keeps `("ab", "c")` and `("a", "bc")` apart.
    pub fn derive(integration: &str, key: &LocalKey) -> Self {
        let mut bytes = Vec::with_capacity(8 + integration.len() + key.as_str().len());
        bytes.extend_from_slice(&(integration.len() as u64).to_le_bytes());
        bytes.extend_from_slice(integration.as_bytes());
        bytes.extend_from_slice(key.as_str().as_bytes());
        Self(twox_hash::XxHash3_128::oneshot(&bytes))
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl std::str::FromStr for NodeId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u128::from_str_radix(s, 16).map(Self)
    }
}

impl Serialize for NodeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_is_a_pure_function_of_integration_and_key() {
        let key = LocalKey::from("0x00158d0001abcd12");
        assert_eq!(NodeId::derive("mqtt", &key), NodeId::derive("mqtt", &key));
        assert_ne!(NodeId::derive("mqtt", &key), NodeId::derive("zigbee", &key));
        assert_ne!(
            NodeId::derive("mqtt", &key),
            NodeId::derive("mqtt", &LocalKey::from("0x00158d0001abcd13"))
        );
    }

    #[test]
    fn the_boundary_between_integration_and_key_is_part_of_the_input() {
        assert_ne!(
            NodeId::derive("ab", &LocalKey::from("c")),
            NodeId::derive("a", &LocalKey::from("bc"))
        );
    }

    #[test]
    fn the_hash_is_pinned() {
        // A change here changes every node id in every deployment, so the
        // value is fixed by test rather than by whatever the crate produces.
        assert_eq!(
            NodeId::derive("wake_on_lan", &LocalKey::from("desktop")).to_string(),
            "773404053e7f4008142ca4166c35b1e7"
        );
    }

    #[test]
    fn ids_round_trip_through_hex_and_json() {
        let id = NodeId::derive("ecoflow", &LocalKey::from("HW52ZEH4XXXX"));
        assert_eq!(id.to_string().parse::<NodeId>().unwrap(), id);

        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<NodeId>(&json).unwrap(), id);

        let mut map = std::collections::HashMap::new();
        map.insert(id, 1);
        let json = serde_json::to_string(&map).unwrap();
        let back: std::collections::HashMap<NodeId, i32> = serde_json::from_str(&json).unwrap();
        assert_eq!(back[&id], 1);
    }
}
