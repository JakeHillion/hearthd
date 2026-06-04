//! Matter-shaped data model used internally and on the public API.
//!
//! hearthd does not currently speak the Matter wire protocol. This module
//! defines a hand-rolled subset of the Matter data model (clusters,
//! attributes, commands, endpoints, nodes) for the device features we
//! currently support. Integration backends translate between their native
//! representations (e.g. Zigbee2MQTT JSON) and these types at their
//! boundary; everything inside hearthd speaks Matter.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

/// Locally assigned Matter node identifier.
pub type NodeId = u64;

/// Endpoint identifier within a node (Matter endpoints are u16).
pub type EndpointId = u16;

// Cluster IDs from the Matter Application Cluster Specification.
pub const CLUSTER_ID_ON_OFF: u32 = 0x0006;
pub const CLUSTER_ID_LEVEL_CONTROL: u32 = 0x0008;
pub const CLUSTER_ID_OCCUPANCY_SENSING: u32 = 0x0406;

pub const CLUSTER_NAME_ON_OFF: &str = "OnOff";
pub const CLUSTER_NAME_LEVEL_CONTROL: &str = "LevelControl";
pub const CLUSTER_NAME_OCCUPANCY_SENSING: &str = "OccupancySensing";

/// On/Off cluster (0x0006).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct OnOffCluster {
    /// Attribute 0x0000 `OnOff`.
    pub on_off: bool,
}

/// Level Control cluster (0x0008).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct LevelControlCluster {
    /// Attribute 0x0000 `CurrentLevel` (0-254, null if unknown).
    pub current_level: Option<u8>,
}

/// Occupancy Sensing cluster (0x0406).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct OccupancySensingCluster {
    /// Attribute 0x0000 `Occupancy` (bit 0 = occupied).
    pub occupancy: bool,
}

/// A Matter cluster instance carrying its current attribute values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, facet::Facet)]
#[serde(tag = "cluster")]
#[repr(u8)]
pub enum Cluster {
    OnOff(OnOffCluster),
    LevelControl(LevelControlCluster),
    OccupancySensing(OccupancySensingCluster),
}

impl Cluster {
    /// Matter cluster ID.
    pub fn id(&self) -> u32 {
        match self {
            Cluster::OnOff(_) => CLUSTER_ID_ON_OFF,
            Cluster::LevelControl(_) => CLUSTER_ID_LEVEL_CONTROL,
            Cluster::OccupancySensing(_) => CLUSTER_ID_OCCUPANCY_SENSING,
        }
    }

    /// Stable name used as the map key inside `Endpoint::clusters`.
    pub fn name(&self) -> &'static str {
        match self {
            Cluster::OnOff(_) => CLUSTER_NAME_ON_OFF,
            Cluster::LevelControl(_) => CLUSTER_NAME_LEVEL_CONTROL,
            Cluster::OccupancySensing(_) => CLUSTER_NAME_OCCUPANCY_SENSING,
        }
    }
}

/// A Matter endpoint: a logical sub-device exposing one or more clusters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, facet::Facet)]
pub struct Endpoint {
    /// Clusters keyed by `Cluster::name()`.
    pub clusters: HashMap<String, Cluster>,
}

/// A Matter node: a physical device addressable on the fabric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, facet::Facet)]
pub struct Node {
    /// External alias used by API clients (e.g. "light.living_room").
    pub entity_id: String,

    /// Name of the integration that owns this node (for command routing).
    pub integration: String,

    /// Human-readable name from discovery, if any.
    pub name: Option<String>,

    /// Endpoints, keyed by endpoint ID.
    pub endpoints: HashMap<EndpointId, Endpoint>,
}

/// OnOff cluster (0x0006) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OnOffCommand {
    /// Command 0x00.
    Off,
    /// Command 0x01.
    On,
    /// Command 0x02.
    Toggle,
}

/// LevelControl cluster (0x0008) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LevelControlCommand {
    /// Command 0x00 `MoveToLevel`.
    MoveToLevel {
        level: u8,
        transition_time: Option<u16>,
    },
}

/// A command to invoke on a cluster. JSON representation:
///   `{"cluster": "OnOff", "command": "On"}`
///   `{"cluster": "LevelControl", "command": {"MoveToLevel": {"level": 200, "transition_time": null}}}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cluster", content = "command")]
pub enum ClusterCommand {
    OnOff(OnOffCommand),
    LevelControl(LevelControlCommand),
}

impl ClusterCommand {
    /// Cluster this command targets.
    pub fn cluster_id(&self) -> u32 {
        match self {
            ClusterCommand::OnOff(_) => CLUSTER_ID_ON_OFF,
            ClusterCommand::LevelControl(_) => CLUSTER_ID_LEVEL_CONTROL,
        }
    }
}
