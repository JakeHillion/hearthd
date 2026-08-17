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

pub use clusters::BatChargeLevel;
pub use clusters::BatChargeState;
pub use clusters::BooleanStateCluster;
pub use clusters::CLUSTER_ID_BOOLEAN_STATE;
pub use clusters::CLUSTER_ID_CLOUD_COVER;
pub use clusters::CLUSTER_ID_COLOR_CONTROL;
pub use clusters::CLUSTER_ID_DEHUMIDIFICATION_CONTROL;
pub use clusters::CLUSTER_ID_DEW_POINT;
pub use clusters::CLUSTER_ID_ELECTRICAL_POWER_MEASUREMENT;
pub use clusters::CLUSTER_ID_FAN_CONTROL;
pub use clusters::CLUSTER_ID_LEVEL_CONTROL;
pub use clusters::CLUSTER_ID_MODE_SELECT;
pub use clusters::CLUSTER_ID_OCCUPANCY_SENSING;
pub use clusters::CLUSTER_ID_ON_OFF;
pub use clusters::CLUSTER_ID_POWER_SOURCE;
pub use clusters::CLUSTER_ID_PRECIPITATION;
pub use clusters::CLUSTER_ID_PRESSURE_MEASUREMENT;
pub use clusters::CLUSTER_ID_RELATIVE_HUMIDITY_MEASUREMENT;
pub use clusters::CLUSTER_ID_TEMPERATURE_MEASUREMENT;
pub use clusters::CLUSTER_ID_THERMOSTAT;
pub use clusters::CLUSTER_ID_THERMOSTAT_USER_INTERFACE_CONFIGURATION;
pub use clusters::CLUSTER_ID_UV_INDEX;
pub use clusters::CLUSTER_ID_WEATHER_CONDITION;
pub use clusters::CLUSTER_ID_WIND_MEASUREMENT;
pub use clusters::CLUSTER_NAME_BOOLEAN_STATE;
pub use clusters::CLUSTER_NAME_CLOUD_COVER;
pub use clusters::CLUSTER_NAME_COLOR_CONTROL;
pub use clusters::CLUSTER_NAME_DEHUMIDIFICATION_CONTROL;
pub use clusters::CLUSTER_NAME_DEW_POINT;
pub use clusters::CLUSTER_NAME_ELECTRICAL_POWER_MEASUREMENT;
pub use clusters::CLUSTER_NAME_FAN_CONTROL;
pub use clusters::CLUSTER_NAME_LEVEL_CONTROL;
pub use clusters::CLUSTER_NAME_MODE_SELECT;
pub use clusters::CLUSTER_NAME_OCCUPANCY_SENSING;
pub use clusters::CLUSTER_NAME_ON_OFF;
pub use clusters::CLUSTER_NAME_POWER_SOURCE;
pub use clusters::CLUSTER_NAME_PRECIPITATION;
pub use clusters::CLUSTER_NAME_PRESSURE_MEASUREMENT;
pub use clusters::CLUSTER_NAME_RELATIVE_HUMIDITY_MEASUREMENT;
pub use clusters::CLUSTER_NAME_TEMPERATURE_MEASUREMENT;
pub use clusters::CLUSTER_NAME_THERMOSTAT;
pub use clusters::CLUSTER_NAME_THERMOSTAT_USER_INTERFACE_CONFIGURATION;
pub use clusters::CLUSTER_NAME_UV_INDEX;
pub use clusters::CLUSTER_NAME_WEATHER_CONDITION;
pub use clusters::CLUSTER_NAME_WIND_MEASUREMENT;
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
pub use clusters::LevelControlCluster;
pub use clusters::ModeOption;
pub use clusters::ModeSelectCluster;
pub use clusters::OccupancySensingCluster;
pub use clusters::OnOffCluster;
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
pub use commands::DehumidificationControlCommand;
pub use commands::FanControlCommand;
pub use commands::LevelControlCommand;
pub use commands::ModeSelectCommand;
pub use commands::OnOffCommand;
pub use commands::SetpointMode;
pub use commands::ThermostatCommand;
pub use commands::ThermostatUserInterfaceConfigurationCommand;

/// Endpoint identifier within a node (Matter endpoints are u16).
pub type EndpointId = u16;

/// A Matter cluster instance carrying its current attribute values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, facet::Facet)]
#[serde(tag = "cluster")]
#[repr(u8)]
pub enum Cluster {
    OnOff(OnOffCluster),
    LevelControl(LevelControlCluster),
    ColorControl(ColorControlCluster),
    TemperatureMeasurement(TemperatureMeasurementCluster),
    RelativeHumidityMeasurement(RelativeHumidityMeasurementCluster),
    OccupancySensing(OccupancySensingCluster),
    BooleanState(BooleanStateCluster),
    Thermostat(ThermostatCluster),
    FanControl(FanControlCluster),
    DehumidificationControl(DehumidificationControlCluster),
    ThermostatUserInterfaceConfiguration(ThermostatUserInterfaceConfigurationCluster),
    PowerSource(PowerSourceCluster),
    ElectricalPowerMeasurement(ElectricalPowerMeasurementCluster),
    ModeSelect(ModeSelectCluster),
    PressureMeasurement(PressureMeasurementCluster),
    WindMeasurement(WindMeasurementCluster),
    CloudCover(CloudCoverCluster),
    DewPoint(DewPointCluster),
    UvIndex(UvIndexCluster),
    Precipitation(PrecipitationCluster),
    WeatherCondition(WeatherConditionCluster),
}

impl Cluster {
    /// Matter cluster ID.
    pub fn id(&self) -> u32 {
        match self {
            Cluster::OnOff(_) => CLUSTER_ID_ON_OFF,
            Cluster::LevelControl(_) => CLUSTER_ID_LEVEL_CONTROL,
            Cluster::ColorControl(_) => CLUSTER_ID_COLOR_CONTROL,
            Cluster::TemperatureMeasurement(_) => CLUSTER_ID_TEMPERATURE_MEASUREMENT,
            Cluster::RelativeHumidityMeasurement(_) => CLUSTER_ID_RELATIVE_HUMIDITY_MEASUREMENT,
            Cluster::OccupancySensing(_) => CLUSTER_ID_OCCUPANCY_SENSING,
            Cluster::BooleanState(_) => CLUSTER_ID_BOOLEAN_STATE,
            Cluster::Thermostat(_) => CLUSTER_ID_THERMOSTAT,
            Cluster::FanControl(_) => CLUSTER_ID_FAN_CONTROL,
            Cluster::DehumidificationControl(_) => CLUSTER_ID_DEHUMIDIFICATION_CONTROL,
            Cluster::ThermostatUserInterfaceConfiguration(_) => {
                CLUSTER_ID_THERMOSTAT_USER_INTERFACE_CONFIGURATION
            }
            Cluster::PowerSource(_) => CLUSTER_ID_POWER_SOURCE,
            Cluster::ElectricalPowerMeasurement(_) => CLUSTER_ID_ELECTRICAL_POWER_MEASUREMENT,
            Cluster::ModeSelect(_) => CLUSTER_ID_MODE_SELECT,
            Cluster::PressureMeasurement(_) => CLUSTER_ID_PRESSURE_MEASUREMENT,
            Cluster::WindMeasurement(_) => CLUSTER_ID_WIND_MEASUREMENT,
            Cluster::CloudCover(_) => CLUSTER_ID_CLOUD_COVER,
            Cluster::DewPoint(_) => CLUSTER_ID_DEW_POINT,
            Cluster::UvIndex(_) => CLUSTER_ID_UV_INDEX,
            Cluster::Precipitation(_) => CLUSTER_ID_PRECIPITATION,
            Cluster::WeatherCondition(_) => CLUSTER_ID_WEATHER_CONDITION,
        }
    }

    /// Stable name used as the map key inside `Endpoint::clusters`.
    pub fn name(&self) -> &'static str {
        match self {
            Cluster::OnOff(_) => CLUSTER_NAME_ON_OFF,
            Cluster::LevelControl(_) => CLUSTER_NAME_LEVEL_CONTROL,
            Cluster::ColorControl(_) => CLUSTER_NAME_COLOR_CONTROL,
            Cluster::TemperatureMeasurement(_) => CLUSTER_NAME_TEMPERATURE_MEASUREMENT,
            Cluster::RelativeHumidityMeasurement(_) => CLUSTER_NAME_RELATIVE_HUMIDITY_MEASUREMENT,
            Cluster::OccupancySensing(_) => CLUSTER_NAME_OCCUPANCY_SENSING,
            Cluster::BooleanState(_) => CLUSTER_NAME_BOOLEAN_STATE,
            Cluster::Thermostat(_) => CLUSTER_NAME_THERMOSTAT,
            Cluster::FanControl(_) => CLUSTER_NAME_FAN_CONTROL,
            Cluster::DehumidificationControl(_) => CLUSTER_NAME_DEHUMIDIFICATION_CONTROL,
            Cluster::ThermostatUserInterfaceConfiguration(_) => {
                CLUSTER_NAME_THERMOSTAT_USER_INTERFACE_CONFIGURATION
            }
            Cluster::PowerSource(_) => CLUSTER_NAME_POWER_SOURCE,
            Cluster::ElectricalPowerMeasurement(_) => CLUSTER_NAME_ELECTRICAL_POWER_MEASUREMENT,
            Cluster::ModeSelect(_) => CLUSTER_NAME_MODE_SELECT,
            Cluster::PressureMeasurement(_) => CLUSTER_NAME_PRESSURE_MEASUREMENT,
            Cluster::WindMeasurement(_) => CLUSTER_NAME_WIND_MEASUREMENT,
            Cluster::CloudCover(_) => CLUSTER_NAME_CLOUD_COVER,
            Cluster::DewPoint(_) => CLUSTER_NAME_DEW_POINT,
            Cluster::UvIndex(_) => CLUSTER_NAME_UV_INDEX,
            Cluster::Precipitation(_) => CLUSTER_NAME_PRECIPITATION,
            Cluster::WeatherCondition(_) => CLUSTER_NAME_WEATHER_CONDITION,
        }
    }
}

/// A Matter endpoint: a logical sub-device exposing one or more clusters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, facet::Facet)]
pub struct Endpoint {
    /// Clusters keyed by `Cluster::name()`.
    pub clusters: HashMap<String, Cluster>,
}

impl Endpoint {
    /// Build an endpoint from a list of clusters, keying each by its name.
    pub fn from_clusters(clusters: impl IntoIterator<Item = Cluster>) -> Self {
        Self {
            clusters: clusters
                .into_iter()
                .map(|c| (c.name().to_string(), c))
                .collect(),
        }
    }
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
