//! What a deployment has to answer for an automation to be linked against it.
//!
//! An `entity_id` is `<domain>.<slug>`. The domain half is a [`Domain`], which
//! every install knows; which slugs exist in a domain is particular to one
//! house and changes as devices are discovered. This trait is the second half:
//! given a domain and a slug, which node — if any — does this deployment have.
//!
//! It is a trait rather than a table so that whatever already holds the
//! information can answer directly. [`crate::engine::state::State`] maintains
//! an `entity_id` index for the API to resolve against; relocation asks the
//! same question, so it asks that index rather than building a second one
//! beside it that could disagree.
//!
//! The checker never sees this. It types `state.light.living_room_lamp`
//! structurally and records the name as a symbol;
//! [`crate::automations::relocate`] resolves those symbols against an index,
//! long after the automation has been compiled. A device that turns up late —
//! Zigbee2MQTT discovery is not instant — calls for a relink, not a recompile.

use crate::automations::domain::Domain;
use crate::engine::NodeId;

/// The entities one deployment has, as relocation needs to see them.
pub trait EntityIndex {
    /// The node `<domain>.<slug>` names, if this deployment has one.
    ///
    /// `None` is an ordinary answer, not a failure: a house with no lights
    /// resolves nothing in the light domain, which is why naming an entity
    /// is a link step and not a type.
    fn resolve(&self, domain: Domain, slug: &str) -> Option<NodeId>;
}
