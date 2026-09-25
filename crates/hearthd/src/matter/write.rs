//! Attribute writes: Matter's second interaction, beside invoking a command.
//!
//! A write sets one attribute to a plain value. It has no parameters beyond
//! the value, no transition semantics and no structured response, which is
//! what separates it from a command in the specification.

use serde::Deserialize;
use serde::Serialize;

/// A write of one attribute on one cluster. JSON representation:
///   `{"cluster": "Thermostat", "attribute": "system_mode", "value": "Cool"}`
///
/// The cluster is named as `Cluster::name()` names it, the attribute as its
/// field on the cluster struct, and the value is that field's JSON encoding,
/// so a write says exactly what `/v1/state` would show once it took effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributeWrite {
    pub cluster: String,
    pub attribute: String,
    pub value: serde_json::Value,
}
