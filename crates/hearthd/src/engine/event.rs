use crate::matter::EndpointId;
use crate::matter::LevelControlCluster;
use crate::matter::Node;
use crate::matter::NodeId;
use crate::matter::OccupancySensingCluster;
use crate::matter::OnOffCluster;

/// Automation-level events.
///
/// Distinct from `FromIntegrationMessage` (transport-level): the engine
/// fans out an `AttributeChanged` message into a per-cluster `Event`
/// variant so DSL programs can read attribute fields directly (e.g.
/// `event.attributes.on_off`).
///
/// The `LightOn` / `LightOff` action variants are intended for emission
/// by observer bodies: when the runner sees one in a body's return value
/// it dispatches the corresponding cluster command back through the
/// engine.
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
    OccupancySensingChanged {
        node_id: NodeId,
        endpoint_id: EndpointId,
        attributes: OccupancySensingCluster,
    },

    /// Action: turn a light on. Carries the target node so the runner
    /// can route the cluster command.
    LightOn(Node),
    /// Action: turn a light off.
    LightOff(Node),
}
