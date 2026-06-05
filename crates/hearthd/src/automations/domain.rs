//! The set of entity domains an `entity_id` can name.
//!
//! An `entity_id` is `<domain>.<slug>` — `light.living_room_lamp`. The
//! domain half is platform knowledge: every HearthD install knows the same
//! domains, whether or not a given deployment has any device in one. The
//! slug half is deployment knowledge, and is what the relocator resolves.
//!
//! Keeping the two apart is what lets `state.light` type check on a house
//! with no lights — yielding an empty group — while `state.lite` is a
//! compile error naming no deployment at all.
//!
//! The variants track the prefixes the integrations construct today. The
//! integrations still build their `entity_id`s with their own `format!`
//! calls, so this is not yet the single point the set is enforced at;
//! making it one is what decides where this type should live.

use std::str::FromStr;

use enum_map::Enum;
use strum::Display;
use strum::EnumIter;
use strum::EnumString;
use strum::IntoStaticStr;

/// A domain an entity can belong to.
///
/// The string form is the `entity_id` prefix, so `Domain::BinarySensor` is
/// `binary_sensor`, and is also the field name an automation writes on
/// `state`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Enum,
    Display,
    EnumString,
    IntoStaticStr,
    EnumIter,
)]
#[strum(serialize_all = "snake_case")]
pub enum Domain {
    BinarySensor,
    Climate,
    Light,
    MediaPlayer,
    Sensor,
    Speaker,
    Weather,
}

impl Domain {
    /// The `entity_id` prefix, which is also the field name on `state`.
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// The domain a field name or `entity_id` prefix names, if it names one.
    pub fn parse(name: &str) -> Option<Self> {
        Self::from_str(name).ok()
    }
}
