use crate::engine::NodeId;
use crate::matter::BooleanStateCluster;
use crate::matter::CloudCoverCluster;
use crate::matter::ColorControlCluster;
use crate::matter::DehumidificationControlCluster;
use crate::matter::DewPointCluster;
use crate::matter::ElectricalPowerMeasurementCluster;
use crate::matter::EndpointId;
use crate::matter::FanControlCluster;
use crate::matter::LevelControlCluster;
use crate::matter::ModeSelectCluster;
use crate::matter::OccupancySensingCluster;
use crate::matter::OnOffCluster;
use crate::matter::PowerSourceCluster;
use crate::matter::PrecipitationCluster;
use crate::matter::PressureMeasurementCluster;
use crate::matter::RelativeHumidityMeasurementCluster;
use crate::matter::TemperatureMeasurementCluster;
use crate::matter::ThermostatCluster;
use crate::matter::ThermostatUserInterfaceConfigurationCluster;
use crate::matter::UvIndexCluster;
use crate::matter::WeatherConditionCluster;
use crate::matter::WindMeasurementCluster;

/// Automation-level events.
///
/// Distinct from `FromIntegrationMessage` (transport-level): the engine
/// fans out an `AttributeChanged` message into a per-cluster `Event`
/// variant so DSL programs can read attribute fields directly (e.g.
/// `event.attributes.on_off`).
#[derive(Debug, Clone)]
pub enum Event {
    OnOffChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: OnOffCluster,
    },
    LevelControlChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: LevelControlCluster,
    },
    ColorControlChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: ColorControlCluster,
    },
    TemperatureMeasurementChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: TemperatureMeasurementCluster,
    },
    PressureMeasurementChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: PressureMeasurementCluster,
    },
    RelativeHumidityMeasurementChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: RelativeHumidityMeasurementCluster,
    },
    OccupancySensingChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: OccupancySensingCluster,
    },
    BooleanStateChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: BooleanStateCluster,
    },
    ThermostatChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: ThermostatCluster,
    },
    FanControlChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: FanControlCluster,
    },
    DehumidificationControlChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: DehumidificationControlCluster,
    },
    ThermostatUserInterfaceConfigurationChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: ThermostatUserInterfaceConfigurationCluster,
    },
    PowerSourceChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: PowerSourceCluster,
    },
    ElectricalPowerMeasurementChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: ElectricalPowerMeasurementCluster,
    },
    ModeSelectChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: ModeSelectCluster,
    },
    WindMeasurementChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: WindMeasurementCluster,
    },
    CloudCoverChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: CloudCoverCluster,
    },
    DewPointChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: DewPointCluster,
    },
    UvIndexChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: UvIndexCluster,
    },
    PrecipitationChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: PrecipitationCluster,
    },
    WeatherConditionChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: WeatherConditionCluster,
    },
}
