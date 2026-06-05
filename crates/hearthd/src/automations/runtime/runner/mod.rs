//! An [`AutomationRunner`] that compiles automations to bytecode and
//! dispatches engine events to them.
//!
//! For each incoming event the runner walks every registered automation,
//! runs its filter synchronously via [`super::vm::run_sync`], and only
//! when a filter passes does it spawn a tokio task to run the body via
//! [`super::vm::run_async`]. A per-automation inflight map holds the
//! current task's join handle so the next firing of the same automation
//! can abort its predecessor — that's how `sleep_unique`'s
//! "most-recent-wins" cancellation is realised.
//!
//! The runner translates returned `Event::LightOn(node)` /
//! `Event::LightOff(node)` action variants into cluster commands and
//! ships them off through an [`ActionSink`]. In production the sink is
//! the [`crate::engine::Engine`] itself; tests can install a recording
//! sink to assert ordering without standing up real integrations.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use tokio::task::JoinHandle;

use super::value::Value;
use super::vm;
use crate::automations::repr::bytecode::Bytecode;
use crate::automations::schema::DeploymentSchema;
use crate::engine::AutomationRunner;
use crate::engine::Engine;
use crate::engine::Event;
use crate::engine::State;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;
use crate::matter::Node;
use crate::matter::NodeId;
use crate::matter::OnOffCommand;

#[cfg(test)]
mod tests;

/// Identifier assigned by the runner to each compiled automation, used
/// to key the inflight map for `sleep_unique` cancellation.
pub type AutomationId = usize;

/// A compiled automation: the filter and body bytecodes plus the kind.
#[derive(Debug, Clone)]
pub struct CompiledAutomation {
    pub id: AutomationId,
    pub kind: crate::automations::repr::ast::AutomationKind,
    pub filter: Bytecode,
    pub body: Bytecode,
}

/// Receives cluster commands the runner has decoded from observer
/// action events. In production this is the [`Engine`]; tests use a
/// recording impl.
pub trait ActionSink: Send + Sync + 'static {
    fn invoke_command(&self, node_id: NodeId, endpoint_id: EndpointId, command: ClusterCommand);
}

impl ActionSink for Engine {
    fn invoke_command(&self, node_id: NodeId, endpoint_id: EndpointId, command: ClusterCommand) {
        if let Err(e) = Engine::invoke_command(self, node_id, endpoint_id, command) {
            tracing::warn!("invoke_command failed: {}", e);
        }
    }
}

/// The runner. Implements [`AutomationRunner`].
pub struct Runner {
    sink: Arc<dyn ActionSink>,
    schema: Arc<DeploymentSchema>,
    automations: Vec<CompiledAutomation>,
    inflight: Mutex<HashMap<AutomationId, JoinHandle<()>>>,
}

impl Runner {
    pub fn new(
        sink: Arc<dyn ActionSink>,
        schema: Arc<DeploymentSchema>,
        automations: Vec<CompiledAutomation>,
    ) -> Self {
        Self {
            sink,
            schema,
            automations,
            inflight: Mutex::new(HashMap::new()),
        }
    }
}

impl AutomationRunner for Runner {
    fn dispatch(&self, event: &Event, state: &Arc<State>) {
        let event_value = event_to_value(event);
        let state_value = state_to_value(state, &self.schema);

        for automation in &self.automations {
            let filter_params = build_params(&automation.filter, &event_value, &state_value);
            let pass = matches!(
                vm::run_sync(&automation.filter, filter_params),
                Ok(Value::Bool(true))
            );
            if !pass {
                continue;
            }

            // `sleep_unique` cancellation: aborting the prior task lets
            // its currently-pending `tokio::time::sleep` future drop, so
            // the only running instance is the most recent one.
            let body = automation.body.clone();
            let body_params = build_params(&body, &event_value, &state_value);
            let sink = self.sink.clone();
            let handle = tokio::spawn(async move {
                match vm::run_async(&body, body_params).await {
                    Ok(result) => dispatch_actions(sink.as_ref(), result),
                    Err(e) => tracing::warn!("automation body failed: {}", e),
                }
            });

            let mut inflight = self.inflight.lock().expect("inflight poisoned");
            if let Some(prior) = inflight.insert(automation.id, handle) {
                prior.abort();
            }
        }
    }
}

// ============================================================================
// Value projection: build VM inputs from engine state + event
// ============================================================================

fn build_params(bc: &Bytecode, event: &Value, state: &Value) -> Vec<Value> {
    bc.params
        .iter()
        .map(|p| match p.name.as_str() {
            "event" => event.clone(),
            "state" => state.clone(),
            other => panic!("unknown top-level param {}", other),
        })
        .collect()
}

fn event_to_value(event: &Event) -> Value {
    let (variant, payload) = match event {
        Event::OnOffChanged {
            node_id,
            endpoint_id,
            attributes,
        } => (
            "OnOffChanged",
            Value::Struct(BTreeMap::from([
                ("node_id".into(), Value::Int(*node_id as i64)),
                ("endpoint_id".into(), Value::Int(*endpoint_id as i64)),
                (
                    "attributes".into(),
                    Value::Struct(BTreeMap::from([(
                        "on_off".into(),
                        Value::Bool(attributes.on_off),
                    )])),
                ),
            ])),
        ),
        Event::LevelControlChanged {
            node_id,
            endpoint_id,
            attributes,
        } => (
            "LevelControlChanged",
            Value::Struct(BTreeMap::from([
                ("node_id".into(), Value::Int(*node_id as i64)),
                ("endpoint_id".into(), Value::Int(*endpoint_id as i64)),
                (
                    "attributes".into(),
                    Value::Struct(BTreeMap::from([(
                        "current_level".into(),
                        match attributes.current_level {
                            Some(v) => Value::Int(v as i64),
                            None => Value::Unit,
                        },
                    )])),
                ),
            ])),
        ),
        Event::OccupancySensingChanged {
            node_id,
            endpoint_id,
            attributes,
        } => (
            "OccupancySensingChanged",
            Value::Struct(BTreeMap::from([
                ("node_id".into(), Value::Int(*node_id as i64)),
                ("endpoint_id".into(), Value::Int(*endpoint_id as i64)),
                (
                    "attributes".into(),
                    Value::Struct(BTreeMap::from([(
                        "occupancy".into(),
                        Value::Bool(attributes.occupancy),
                    )])),
                ),
            ])),
        ),
        Event::LightOn(node) => ("LightOn", node_to_value(node)),
        Event::LightOff(node) => ("LightOff", node_to_value(node)),
    };
    Value::Variant {
        enum_name: "Event".into(),
        variant: variant.into(),
        args: vec![payload],
    }
}

fn state_to_value(state: &Arc<State>, schema: &DeploymentSchema) -> Value {
    let mut domains = BTreeMap::new();
    for (domain, slugs) in &schema.domains {
        let mut slug_fields = BTreeMap::new();
        for (slug, node_id) in slugs {
            if let Some(node) = state.nodes.get(node_id) {
                slug_fields.insert(slug.clone(), node_to_value(node));
            }
        }
        domains.insert(domain.clone(), Value::Struct(slug_fields));
    }
    Value::Struct(domains)
}

fn node_to_value(node: &Node) -> Value {
    Value::Struct(BTreeMap::from([
        ("id".into(), Value::Int(node.id as i64)),
        ("entity_id".into(), Value::String(node.entity_id.clone())),
        (
            "integration".into(),
            Value::String(node.integration.clone()),
        ),
    ]))
}

// ============================================================================
// Action dispatch: translate body return values into cluster commands
// ============================================================================

fn dispatch_actions(sink: &dyn ActionSink, result: Value) {
    let Value::List(items) = result else {
        tracing::warn!(
            "observer body must return a list of Events, got {:?}",
            result
        );
        return;
    };
    for item in items {
        let Value::Variant {
            enum_name,
            variant,
            args,
        } = item
        else {
            tracing::warn!("ignoring non-event in body output");
            continue;
        };
        if enum_name != "Event" {
            continue;
        }
        // Endpoint 1 is the Z2M convention for the application endpoint
        // on every node we currently surface; once the engine carries
        // richer endpoint metadata the runner should look up the cluster
        // location on the node instead of hard-coding 1.
        let endpoint: EndpointId = 1;
        match (variant.as_str(), args.as_slice()) {
            ("LightOn", [Value::Struct(node_fields)]) => {
                if let Some(Value::Int(id)) = node_fields.get("id") {
                    sink.invoke_command(
                        *id as NodeId,
                        endpoint,
                        ClusterCommand::OnOff(OnOffCommand::On),
                    );
                }
            }
            ("LightOff", [Value::Struct(node_fields)]) => {
                if let Some(Value::Int(id)) = node_fields.get("id") {
                    sink.invoke_command(
                        *id as NodeId,
                        endpoint,
                        ClusterCommand::OnOff(OnOffCommand::Off),
                    );
                }
            }
            _ => {
                // Other action variants aren't supported yet.
            }
        }
    }
}
