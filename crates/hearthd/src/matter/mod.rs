//! Matter-shaped data model used internally and on the public API.
//!
//! hearthd does not currently speak the Matter wire protocol. This module
//! defines a hand-rolled subset of the Matter data model (clusters,
//! attributes, commands, endpoints, nodes) for the device features we
//! currently support. Integration backends translate between their native
//! representations (e.g. Zigbee2MQTT JSON, EcoFlow protobuf) and these types
//! at their boundary; everything inside hearthd speaks Matter.
//!
//! # Endpoints
//!
//! Matter identifies a cluster instance by the pair (endpoint ID, cluster ID);
//! there is no instance discriminator. A device with several sensors of the
//! same kind therefore has to spread them across several endpoints — six
//! thermistors means six endpoints, each carrying one
//! `TemperatureMeasurementCluster`. That is the specification's rule, not a
//! hearthd limitation, and it is why integrations for physically rich devices
//! produce endpoint counts in the dozens. Endpoint IDs are `u16` with 0
//! reserved for the root node, so there is ample room.
//!
//! `Endpoint::clusters` is keyed by `Cluster::name()` rather than by cluster
//! ID. That is looser than Matter — it makes a display name load-bearing — but
//! it enforces the same one-instance-per-endpoint invariant.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

mod clusters;
mod commands;
mod device_types;
mod semantic_tags;
mod write;

pub use clusters::BatChargeLevel;
pub use clusters::BatChargeState;
pub use clusters::BatteryStorageControlCluster;
pub use clusters::BooleanStateCluster;
pub use clusters::CloudCoverCluster;
pub use clusters::ColorControlCluster;
pub use clusters::ColorControlOptions;
pub use clusters::ColorMode;
pub use clusters::ControlSequenceOfOperation;
pub use clusters::DehumidificationControlCluster;
pub use clusters::DewPointCluster;
pub use clusters::ElectricalPowerMeasurementCluster;
pub use clusters::FanControlCluster;
pub use clusters::FanMode;
pub use clusters::FanModeSequence;
pub use clusters::InputInfo;
pub use clusters::InputType;
pub use clusters::LevelControlCluster;
pub use clusters::MediaInputCluster;
pub use clusters::MediaPlaybackCluster;
pub use clusters::ModeOption;
pub use clusters::ModeSelectCluster;
pub use clusters::OccupancySensingCluster;
pub use clusters::OnOffCluster;
pub use clusters::PlaybackState;
pub use clusters::PowerMode;
pub use clusters::PowerSourceCluster;
pub use clusters::PowerSourceStatus;
pub use clusters::PrecipitationCluster;
pub use clusters::PressureMeasurementCluster;
pub use clusters::RelativeHumidityMeasurementCluster;
pub use clusters::SystemMode;
pub use clusters::TemperatureDisplayMode;
pub use clusters::TemperatureMeasurementCluster;
pub use clusters::ThermostatCluster;
pub use clusters::ThermostatUserInterfaceConfigurationCluster;
pub use clusters::UvIndexCluster;
pub use clusters::WeatherCondition;
pub use clusters::WeatherConditionCluster;
pub use clusters::WindMeasurementCluster;
pub use commands::ClusterCommand;
pub use commands::ColorControlCommand;
pub use commands::LevelControlCommand;
pub use commands::MediaInputCommand;
pub use commands::MediaPlaybackCommand;
pub use commands::ModeSelectCommand;
pub use commands::OnOffCommand;
pub use commands::SetpointMode;
pub use commands::ThermostatCommand;
pub use device_types::DeviceType;
pub use semantic_tags::*;
pub use write::AttributeWrite;

/// Endpoint identifier within a node (Matter endpoints are u16).
pub type EndpointId = u16;

/// A Matter cluster instance carrying its current attribute values.
///
/// Each variant's discriminant is its cluster ID and its name is the cluster's
/// name, so the compiler rejects two clusters sharing either. hearthd-local
/// clusters, for quantities Matter does not (yet) model, take IDs from the
/// manufacturer-specific range (0xFC00-0xFFFE); if Matter later standardises
/// one, swap it for the official cluster.
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, facet::Facet, strum::EnumDiscriminants,
)]
#[strum_discriminants(
    name(ClusterId),
    derive(
        Hash,
        strum::EnumString,
        strum::FromRepr,
        strum::IntoStaticStr,
        strum::VariantArray
    )
)]
#[serde(tag = "cluster")]
#[repr(u32)]
pub enum Cluster {
    OnOff(OnOffCluster) = 0x0006,
    LevelControl(LevelControlCluster) = 0x0008,
    ColorControl(ColorControlCluster) = 0x0300,
    TemperatureMeasurement(TemperatureMeasurementCluster) = 0x0402,
    RelativeHumidityMeasurement(RelativeHumidityMeasurementCluster) = 0x0405,
    OccupancySensing(OccupancySensingCluster) = 0x0406,
    BooleanState(BooleanStateCluster) = 0x0045,
    Thermostat(ThermostatCluster) = 0x0201,
    FanControl(FanControlCluster) = 0x0202,
    DehumidificationControl(DehumidificationControlCluster) = 0x0203,
    ThermostatUserInterfaceConfiguration(ThermostatUserInterfaceConfigurationCluster) = 0x0204,
    PowerSource(PowerSourceCluster) = 0x002F,
    ElectricalPowerMeasurement(ElectricalPowerMeasurementCluster) = 0x0090,
    ModeSelect(ModeSelectCluster) = 0x0050,
    PressureMeasurement(PressureMeasurementCluster) = 0x0403,
    MediaPlayback(MediaPlaybackCluster) = 0x0506,
    MediaInput(MediaInputCluster) = 0x0507,
    WindMeasurement(WindMeasurementCluster) = 0xFC00,
    CloudCover(CloudCoverCluster) = 0xFC01,
    DewPoint(DewPointCluster) = 0xFC02,
    UvIndex(UvIndexCluster) = 0xFC03,
    Precipitation(PrecipitationCluster) = 0xFC04,
    WeatherCondition(WeatherConditionCluster) = 0xFC05,
    BatteryStorageControl(BatteryStorageControlCluster) = 0xFC06,
}

const _: () = {
    let ids = <ClusterId as strum::VariantArray>::VARIANTS;
    let mut i = 0;
    while i < ids.len() {
        let id = ids[i] as u32;
        assert!(
            id <= 0x7FFF || (id >= 0xFC00 && id <= 0xFFFE),
            "cluster IDs are standard (0x0000-0x7FFF) or manufacturer-specific (0xFC00-0xFFFE)"
        );
        i += 1;
    }
};

impl Cluster {
    /// Matter cluster ID.
    pub fn id(&self) -> ClusterId {
        self.into()
    }

    /// Stable name used as the map key inside `Endpoint::clusters`.
    pub fn name(&self) -> &'static str {
        self.id().name()
    }
}

impl ClusterId {
    /// The cluster's name, as `Cluster::name()` gives it.
    pub fn name(self) -> &'static str {
        self.into()
    }
}

/// A Matter endpoint: a logical sub-device exposing one or more clusters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, facet::Facet)]
pub struct Endpoint {
    /// What this endpoint is, as its Descriptor cluster's `DeviceTypeList`
    /// would say. Each type names the clusters the endpoint must carry; an
    /// endpoint may carry more, and one with no fitting type declares none.
    pub device_types: Vec<DeviceType>,

    /// What tells this endpoint apart from its siblings, as its Descriptor
    /// cluster's `TagList` would say. Matter allows at most six, and requires
    /// them on every endpoint that shares a device type with a sibling; see
    /// `Node::ambiguous_endpoints`.
    pub tags: Vec<SemanticTag>,

    /// Clusters keyed by `Cluster::name()`.
    pub clusters: HashMap<String, Cluster>,
}

/// A `SemanticTagStruct`: a tag from a standard namespace, optionally with a
/// human-readable label.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, facet::Facet)]
pub struct SemanticTag {
    #[serde(flatten)]
    pub tag: Tag,

    /// At most 64 bytes, for display.
    pub label: Option<String>,
}

impl From<Tag> for SemanticTag {
    fn from(tag: Tag) -> Self {
        Self { tag, label: None }
    }
}

impl Endpoint {
    /// Build an endpoint from a list of clusters, keying each by its name.
    pub fn from_clusters(clusters: impl IntoIterator<Item = Cluster>) -> Self {
        Self {
            device_types: Vec::new(),
            tags: Vec::new(),
            clusters: clusters
                .into_iter()
                .map(|c| (c.name().to_string(), c))
                .collect(),
        }
    }

    /// Declare the endpoint's device types.
    pub fn with_device_types(mut self, device_types: impl IntoIterator<Item = DeviceType>) -> Self {
        self.device_types = device_types.into_iter().collect();
        self
    }

    /// Declare the endpoint's semantic tags.
    pub fn with_tags(mut self, tags: impl IntoIterator<Item = SemanticTag>) -> Self {
        self.tags = tags.into_iter().collect();
        self
    }

    /// Clusters a declared device type requires that the endpoint lacks, as
    /// (type, cluster ID) pairs.
    ///
    /// Only clusters hearthd models count: the Device Library also mandates
    /// Identify and Groups on nearly everything, and an endpoint cannot carry
    /// a cluster there is no type for.
    pub fn missing_mandatory_clusters(&self) -> Vec<(DeviceType, u32)> {
        let present: Vec<ClusterId> = self.clusters.values().map(Cluster::id).collect();
        self.device_types
            .iter()
            .flat_map(|device_type| {
                device_type
                    .mandatory_clusters()
                    .iter()
                    .filter(|id| {
                        ClusterId::from_repr(**id).is_some_and(|id| !present.contains(&id))
                    })
                    .map(move |id| (*device_type, *id))
            })
            .collect()
    }
}

/// The owning integration's own stable name for a node: a Zigbee address, a
/// serial number, a config key. Paired with the integration's name it is what
/// identifies the node; the integration is never told anything else.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, facet::Facet,
)]
#[serde(transparent)]
pub struct LocalKey(String);

impl LocalKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for LocalKey {
    fn from(key: String) -> Self {
        Self(key)
    }
}

impl From<&str> for LocalKey {
    fn from(key: &str) -> Self {
        Self(key.to_string())
    }
}

impl std::fmt::Display for LocalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A Matter node: a physical device addressable on the fabric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, facet::Facet)]
pub struct Node {
    /// The owning integration's name for this node.
    pub key: LocalKey,

    /// Human-readable name from discovery, if any.
    pub name: Option<String>,

    /// Endpoints, keyed by endpoint ID.
    pub endpoints: HashMap<EndpointId, Endpoint>,
}

impl Node {
    /// Endpoints whose tags do not tell them apart from a sibling of the same
    /// device type, as (endpoint, type) pairs in endpoint order.
    ///
    /// Matter requires every endpoint that shares a device type with a
    /// sibling to carry a tag list, and no two of them to carry the same one.
    pub fn ambiguous_endpoints(&self) -> Vec<(EndpointId, DeviceType)> {
        let mut ambiguous = Vec::new();
        for (id, endpoint) in &self.endpoints {
            for device_type in &endpoint.device_types {
                let clashes = self.endpoints.iter().any(|(other_id, other)| {
                    other_id != id
                        && other.device_types.contains(device_type)
                        && (endpoint.tags.is_empty() || same_tags(&endpoint.tags, &other.tags))
                });
                if clashes {
                    ambiguous.push((*id, *device_type));
                }
            }
        }
        ambiguous.sort_by_key(|(id, device_type)| (*id, device_type.id()));
        ambiguous
    }
}

/// Whether two tag lists hold the same tags, in any order.
fn same_tags(a: &[SemanticTag], b: &[SemanticTag]) -> bool {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort();
    b.sort();
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_endpoint_carrying_its_mandatory_clusters_is_conformant() {
        let endpoint = Endpoint::from_clusters([
            Cluster::OnOff(OnOffCluster::default()),
            Cluster::LevelControl(LevelControlCluster::default()),
        ])
        .with_device_types([DeviceType::Speaker]);

        assert_eq!(endpoint.missing_mandatory_clusters(), []);
    }

    #[test]
    fn a_missing_mandatory_cluster_is_reported_against_its_type() {
        let endpoint = Endpoint::from_clusters([Cluster::OnOff(OnOffCluster::default())])
            .with_device_types([DeviceType::OnOffLight, DeviceType::Speaker]);

        assert_eq!(
            endpoint.missing_mandatory_clusters(),
            [(DeviceType::Speaker, ClusterId::LevelControl as u32)]
        );
    }

    #[test]
    fn clusters_hearthd_does_not_model_are_never_missing() {
        // Every light type mandates Identify and Groups, which no endpoint
        // can carry, so an On/Off Light with only On/Off is conformant.
        let endpoint = Endpoint::from_clusters([Cluster::OnOff(OnOffCluster::default())])
            .with_device_types([DeviceType::OnOffLight]);

        assert!(
            DeviceType::OnOffLight
                .mandatory_clusters()
                .contains(&0x0003)
        );
        assert_eq!(endpoint.missing_mandatory_clusters(), []);
    }

    #[test]
    fn extra_clusters_do_not_break_conformance() {
        let endpoint = Endpoint::from_clusters([
            Cluster::BooleanState(BooleanStateCluster::default()),
            Cluster::OnOff(OnOffCluster::default()),
        ])
        .with_device_types([DeviceType::ContactSensor]);

        assert_eq!(endpoint.missing_mandatory_clusters(), []);
    }

    fn sensor(tags: impl IntoIterator<Item = SemanticTag>) -> Endpoint {
        Endpoint::from_clusters([Cluster::TemperatureMeasurement(
            TemperatureMeasurementCluster::default(),
        )])
        .with_device_types([DeviceType::TemperatureSensor])
        .with_tags(tags)
    }

    fn node(endpoints: impl IntoIterator<Item = (EndpointId, Endpoint)>) -> Node {
        Node {
            key: LocalKey::from("test"),
            name: None,
            endpoints: endpoints.into_iter().collect(),
        }
    }

    const INDOOR: Tag = Tag::CommonLocation(CommonLocationTag::Indoor);
    const OUTDOOR: Tag = Tag::CommonLocation(CommonLocationTag::Outdoor);

    #[test]
    fn a_lone_device_type_needs_no_tags() {
        let node = node([
            (1, sensor([])),
            (
                2,
                Endpoint::from_clusters([Cluster::OnOff(OnOffCluster::default())])
                    .with_device_types([DeviceType::OnOffLight]),
            ),
        ]);

        assert_eq!(node.ambiguous_endpoints(), []);
    }

    #[test]
    fn siblings_sharing_a_device_type_need_tags() {
        let node = node([(1, sensor([INDOOR.into()])), (2, sensor([]))]);

        assert_eq!(
            node.ambiguous_endpoints(),
            [(2, DeviceType::TemperatureSensor)]
        );
    }

    #[test]
    fn siblings_with_the_same_tags_are_ambiguous_in_any_order() {
        let node = node([
            (1, sensor([INDOOR.into(), OUTDOOR.into()])),
            (2, sensor([OUTDOOR.into(), INDOOR.into()])),
        ]);

        assert_eq!(
            node.ambiguous_endpoints(),
            [
                (1, DeviceType::TemperatureSensor),
                (2, DeviceType::TemperatureSensor)
            ]
        );
    }

    #[test]
    fn a_label_tells_siblings_apart() {
        let labelled = SemanticTag {
            tag: INDOOR,
            label: Some("Hallway".to_string()),
        };
        let node = node([(1, sensor([INDOOR.into()])), (2, sensor([labelled]))]);

        assert_eq!(node.ambiguous_endpoints(), []);
    }

    #[test]
    fn a_tag_serializes_by_namespace_and_tag_name() {
        let tag = SemanticTag {
            tag: Tag::RoomAirConditioner(RoomAirConditionerTag::Condenser),
            label: None,
        };

        let json = serde_json::to_value(&tag).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "namespace": "RoomAirConditioner",
                "tag": "Condenser",
                "label": null,
            })
        );
        assert_eq!(serde_json::from_value::<SemanticTag>(json).unwrap(), tag);
    }
}
