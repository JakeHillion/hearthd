//! Attribute writes targeting a cluster endpoint.
//!
//! Matter controls most device state by *writing attributes* rather than by
//! invoking cluster commands. This module models those writes as a first-class
//! operation distinct from [`ClusterCommand`].
//!
//! Each write type covers the writable attributes of one cluster. Fields are
//! `Option<T>` so a single request can update any subset: `Some(v)` means "set
//! this attribute", `None` means "leave it alone".
//!
//! [`ClusterCommand`]: super::commands::ClusterCommand

use serde::Deserialize;
use serde::Serialize;

use super::clusters::CLUSTER_ID_THERMOSTAT;
use super::clusters::SystemMode;

/// Attribute write for the Thermostat cluster (0x0201).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ThermostatWrite {
    /// Attribute 0x001C `SystemMode`.
    pub system_mode: Option<SystemMode>,

    /// Attribute 0x0011 `OccupiedCoolingSetpoint`, in hundredths of a degree
    /// Celsius. In `SystemMode::Auto` this is the range's upper bound.
    pub occupied_cooling_setpoint: Option<i16>,

    /// Attribute 0x0012 `OccupiedHeatingSetpoint`, in hundredths of a degree
    /// Celsius. In `SystemMode::Auto` this is the range's lower bound.
    pub occupied_heating_setpoint: Option<i16>,
}

/// A set of attribute writes to apply to one cluster.
///
/// JSON representation:
///   `{"cluster": "Thermostat", "system_mode": "Heat"}`
///   `{"cluster": "Thermostat", "occupied_heating_setpoint": 2100}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cluster")]
pub enum ClusterWrite {
    Thermostat(ThermostatWrite),
}

impl ClusterWrite {
    /// Cluster this write targets.
    pub fn cluster_id(&self) -> u32 {
        match self {
            ClusterWrite::Thermostat(_) => CLUSTER_ID_THERMOSTAT,
        }
    }

    /// Stable name used as the map key inside [`crate::matter::Endpoint::clusters`].
    pub fn cluster_name(&self) -> &'static str {
        match self {
            ClusterWrite::Thermostat(_) => crate::matter::CLUSTER_NAME_THERMOSTAT,
        }
    }

    /// True if the write carries at least one `Some` field.
    pub fn has_any_field(&self) -> bool {
        match self {
            ClusterWrite::Thermostat(t) => {
                t.system_mode.is_some()
                    || t.occupied_cooling_setpoint.is_some()
                    || t.occupied_heating_setpoint.is_some()
            }
        }
    }
}
