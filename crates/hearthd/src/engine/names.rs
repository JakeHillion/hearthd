//! The names a node answers to.
//!
//! A node has one identity, its [`NodeId`](super::NodeId), and any number of
//! names. Its integration gives it one, the name the upstream system knows it
//! by, slugged into an identifier by [`slug`]. Configuration can add aliases.
//! Both feed one table, and the engine resolves a name by alias first, then
//! by a discovered name that exactly one node carries, then as a hex id.

use super::NodeId;

/// Why a name did not resolve to a node.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("unknown node: {0}")]
    Unknown(String),

    /// More than one node was discovered under the name. An alias for the
    /// one meant picks it.
    #[error("{name} names more than one node: {}", ids_for_display(.ids))]
    Ambiguous { name: String, ids: Vec<NodeId> },
}

fn ids_for_display(ids: &[NodeId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The address form of a name an integration gave a node.
///
/// Lowercase, with every run of characters outside `[a-z0-9]` collapsed to
/// one underscore and none left at either end, so `Living Room Lamp` is
/// addressed as `living_room_lamp`. Empty when the name had nothing to keep.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_collapses_to_an_identifier() {
        assert_eq!(slug("Living Room Lamp"), "living_room_lamp");
        assert_eq!(slug("  Kitchen -- Speaker! "), "kitchen_speaker");
        assert_eq!(slug("bedroom_ac"), "bedroom_ac");
        assert_eq!(slug("0x00158d0001abcd12"), "0x00158d0001abcd12");
    }

    #[test]
    fn slug_of_nothing_addressable_is_empty() {
        assert_eq!(slug("!!"), "");
        assert_eq!(slug(""), "");
    }
}
