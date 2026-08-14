use crate::matter::EndpointId;
use crate::matter::LevelControlCluster;
use crate::matter::NodeId;
use crate::matter::OccupancySensingCluster;
use crate::matter::OnOffCluster;
use crate::matter::TemperatureMeasurementCluster;

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
    OccupancySensingChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: OccupancySensingCluster,
    },
}
