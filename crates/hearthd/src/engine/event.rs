use crate::engine::NodeId;
use crate::matter::BooleanStateCluster;
use crate::matter::DehumidificationControlCluster;
use crate::matter::ElectricalPowerMeasurementCluster;
use crate::matter::EndpointId;
use crate::matter::FanControlCluster;
use crate::matter::LevelControlCluster;
use crate::matter::ModeSelectCluster;
use crate::matter::OccupancySensingCluster;
use crate::matter::OnOffCluster;
use crate::matter::PowerSourceCluster;
use crate::matter::RelativeHumidityMeasurementCluster;
use crate::matter::TemperatureMeasurementCluster;
use crate::matter::ThermostatCluster;
use crate::matter::ThermostatUserInterfaceConfigurationCluster;

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
    TemperatureMeasurementChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: TemperatureMeasurementCluster,
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
}
