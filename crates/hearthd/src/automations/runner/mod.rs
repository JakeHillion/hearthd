//! The [`AutomationRunner`] implementation: engine events in, cluster
//! commands out.
//!
//! One [`Runner`] holds every deployed automation. For each event it runs
//! every filter synchronously, on the engine's own task — that is what
//! [`Vm::run_sync`] is built for, and it keeps an event matching nothing free
//! of task spawns. Only a filter that passes costs one, and the body runs
//! there because it may suspend.
//!
//! Firings run side by side. Each one gets its own [`Vm::instance`] — its own
//! register file over the one shared program — and its own task, and nothing
//! cuts an earlier one short. That is `sleep`'s contract: a body waiting on
//! one runs to its end, and a firing that arrives meanwhile runs beside it
//! rather than replacing it.
//!
//! `sleep_unique` is the one that yields, and it yields by failing rather
//! than by dying. Each automation keeps a generation counter that every
//! firing increments; an instance holds the number it was given, and its
//! `sleep_unique` resolves `false` as soon as the counter passes it. The body
//! is not killed — it carries on into its `else` branch and returns whatever
//! that yields. Killing it would make the `else` unreachable and the `Bool`
//! in `Future<Bool>` pointless.
//!
//! Nothing bounds how many instances of one automation can be alive at once,
//! which is the cost of `sleep` being uninterruptible: an automation that
//! sleeps for an hour and fires every minute accumulates sixty bodies. That
//! is the semantics asked for rather than an oversight, and a bound would be
//! a policy this does not have one for yet.
//!
//! What a body returns is a list of action `Event`s, which the runner decodes
//! into cluster commands and hands to an [`ActionSink`]. In production that
//! sink is the [`Engine`]; a test installs a recording one and asserts on the
//! commands rather than standing up integrations.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use tokio::sync::watch;

use crate::automations::repr::ast::AutomationKind;
use crate::automations::repr::bytecode::Bytecode;
use crate::automations::repr::bytecode::BytecodeParam;
use crate::automations::schema::DeploymentSchema;
use crate::automations::vm::Pending;
use crate::automations::vm::Suspension;
use crate::automations::vm::Value;
use crate::automations::vm::Vm;
use crate::automations::vm::VmError;
use crate::engine::AutomationRunner;
use crate::engine::Engine;
use crate::engine::Event;
use crate::engine::NodeId;
use crate::engine::State;
use crate::matter::ClusterCommand;
use crate::matter::EndpointId;
use crate::matter::Node;
use crate::matter::OnOffCommand;

#[cfg(test)]
mod tests;

/// The endpoint a cluster command is addressed to.
///
/// Every node hearthd surfaces today carries its application clusters on
/// endpoint 1, which is the convention the integrations follow. Once a `Node`
/// records where a cluster actually lives, the runner should read it off the
/// node rather than assume.
const ACTION_ENDPOINT: EndpointId = 1;

/// Which of the runner's two inputs a declared parameter binds.
///
/// Resolved when the automation is deployed rather than by matching the
/// parameter's name on every event — the same reason a `Vm` decodes its
/// constant pool up front. It also turns a parameter the runner has nothing
/// to supply into a deployment failure instead of a per-event one.
#[derive(Debug, Clone, Copy)]
enum Binding {
    Event,
    State,
}

impl Binding {
    fn resolve(params: &[BytecodeParam]) -> Result<Box<[Binding]>, VmError> {
        params
            .iter()
            .map(|param| match param.name.as_str() {
                "event" => Ok(Binding::Event),
                "state" => Ok(Binding::State),
                // An observer binds `event` and `state` and nothing else, so
                // another name means the compiler produced a function the
                // runner cannot call.
                other => Err(VmError::InvariantViolation(format!(
                    "automation binds `{}`, which the runner cannot supply",
                    other
                ))),
            })
            .collect()
    }
}

/// One deployed automation, compiled and ready to run.
pub struct CompiledAutomation {
    kind: AutomationKind,
    /// The filter, behind a lock because a `Vm` is rebound and rerun in place
    /// while `dispatch` only has `&self`. Held rather than cloned per event:
    /// being built once and rerun is the whole point of a filter's `Vm`.
    filter: Mutex<Vm>,
    filter_bindings: Box<[Binding]>,
    /// The body, kept as a template that each firing takes an instance of. A
    /// suspended function owns its registers, so it cannot be shared the way
    /// a filter is; the program behind it is shared, so an instance costs one
    /// register file rather than a copy of the bytecode.
    body: Vm,
    body_bindings: Box<[Binding]>,
    /// Counts firings, so an instance can tell whether a newer one has
    /// started. Only ever increases, and only the newest value matters, which
    /// is what makes a `watch` the right channel: an instance that was
    /// superseded twice over still just sees a number above its own.
    generation: watch::Sender<u64>,
}

impl CompiledAutomation {
    /// Build both of an automation's functions.
    ///
    /// Fails if either carries a unit literal that does not fit its
    /// dimension's canonical unit, or declares a parameter the runner cannot
    /// supply. Both are caught when the automation is deployed rather than on
    /// the first event that reaches them.
    pub fn new(kind: AutomationKind, filter: Bytecode, body: Bytecode) -> Result<Self, VmError> {
        let filter_bindings = Binding::resolve(&filter.params)?;
        let body_bindings = Binding::resolve(&body.params)?;
        Ok(Self {
            kind,
            filter: Mutex::new(Vm::new(filter)?),
            filter_bindings,
            body: Vm::new(body)?,
            body_bindings,
            generation: watch::Sender::new(0),
        })
    }

    /// Start a firing: take the next generation and the view of the counter
    /// that goes with it.
    ///
    /// Incrementing before the body runs is what makes the new instance the
    /// newest: it compares equal to the counter, where every instance already
    /// running compares below it.
    fn begin(&self) -> Instance {
        let mut generation = 0;
        self.generation.send_modify(|current| {
            *current += 1;
            generation = *current;
        });
        Instance {
            generation,
            newest: self.generation.subscribe(),
        }
    }
}

/// One firing of one automation, and its view of whether a newer one has
/// started.
struct Instance {
    /// The generation this firing was given.
    generation: u64,
    /// The newest generation of the same automation.
    newest: watch::Receiver<u64>,
}

#[async_trait]
impl Suspension for Instance {
    async fn resolve(&self, pending: Pending) -> Value {
        match pending {
            // Nothing interrupts a `sleep`, whatever else has started since.
            Pending::Sleep(_) => {
                tokio::time::sleep(pending.duration()).await;
                Value::Unit
            }
            Pending::SleepUnique(_) => {
                let mut newest = self.newest.clone();
                let generation = self.generation;
                tokio::select! {
                    _ = tokio::time::sleep(pending.duration()) => Value::Bool(true),
                    // `wait_for` tests the current value before waiting at
                    // all, so an instance superseded while it sat in an
                    // earlier `sleep` fails here without serving any of this
                    // wait. That is the one case the two rules above do not
                    // dictate between them, and this is the reading they
                    // compose to: the `sleep` ran because nothing may cut one
                    // short, and the `sleep_unique` fails because by the time
                    // it is reached a newer instance already exists.
                    _ = async {
                        if newest.wait_for(|newest| *newest > generation).await.is_err() {
                            // The automation is gone, so no firing can follow
                            // this one; leave the wait to decide.
                            std::future::pending::<()>().await
                        }
                    } => Value::Bool(false),
                }
            }
        }
    }
}

/// Receives the cluster commands the runner decodes out of body results.
///
/// The seam exists so the runner can be driven without an engine: the
/// production sink is the [`Engine`], and a test installs one that records.
pub trait ActionSink: Send + Sync + 'static {
    fn invoke_command(&self, node_id: NodeId, endpoint_id: EndpointId, command: ClusterCommand);
}

impl ActionSink for Engine {
    fn invoke_command(&self, node_id: NodeId, endpoint_id: EndpointId, command: ClusterCommand) {
        // Reported rather than propagated: an automation naming a node no
        // integration owns is that automation's problem, not grounds for
        // failing the event that triggered it.
        if let Err(e) = Engine::invoke_command(self, node_id, endpoint_id, command) {
            tracing::warn!("automation command failed: {}", e);
        }
    }
}

/// Every deployed automation, and the bodies currently in flight.
pub struct Runner {
    sink: Arc<dyn ActionSink>,
    schema: Arc<DeploymentSchema>,
    automations: Vec<CompiledAutomation>,
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
        }
    }

    /// Run one automation's filter. Anything but a clean `true` leaves the
    /// automation quiet: a filter that fails must not fire the body it
    /// guards.
    fn filter_passes(&self, automation: &CompiledAutomation, event: &Value, state: &Value) -> bool {
        let mut filter = automation.filter.lock().expect("filter lock poisoned");
        let params = bind(&automation.filter_bindings, event, state);
        match filter.run_sync(params) {
            Ok(Value::Bool(pass)) => pass,
            Ok(other) => {
                tracing::warn!("automation filter returned {}, not a boolean", other);
                false
            }
            Err(e) => {
                tracing::warn!("automation filter failed: {}", e);
                false
            }
        }
    }
}

impl AutomationRunner for Runner {
    fn dispatch(&self, event: &Event, state: &Arc<State>) {
        let event_value = event_to_value(event);
        let state_value = state_to_value(state, &self.schema);

        for automation in &self.automations {
            // A mutator is not event-triggered. Nothing compiles one into a
            // runner yet; skipping is what keeps that true if something does.
            if automation.kind != AutomationKind::Observer {
                continue;
            }
            if !self.filter_passes(automation, &event_value, &state_value) {
                continue;
            }

            // Taking the generation before spawning is what supersedes the
            // instances already running: their `sleep_unique`s see the
            // counter pass them and resolve `false` while they are still
            // parked. Nothing is aborted, and the handle is dropped because
            // there is nothing left to do with it.
            let instance = automation.begin();
            let body = automation.body.instance();
            let params = bind(&automation.body_bindings, &event_value, &state_value);
            let sink = self.sink.clone();
            tokio::spawn(async move {
                match body.run_async(params, &instance).await {
                    Ok(result) => dispatch_actions(sink.as_ref(), result),
                    Err(e) => tracing::warn!("automation body failed: {}", e),
                }
            });
        }
    }
}

/// Build a parameter list in the order the function declared it.
fn bind(bindings: &[Binding], event: &Value, state: &Value) -> Vec<Value> {
    bindings
        .iter()
        .map(|binding| match binding {
            Binding::Event => event.clone(),
            Binding::State => state.clone(),
        })
        .collect()
}

// ============================================================================
// Projection: engine values into automation values
// ============================================================================

/// Project an engine event into the value an automation sees.
///
/// Every `*Changed` variant has one shape — a node, an endpoint and a cluster
/// snapshot — so they are generated from a list of names rather than written
/// out twenty-three times over. The list is still exhaustive: adding an
/// `Event` variant without naming it here does not compile.
macro_rules! project_events {
    ($event:expr, $($variant:ident),+ $(,)?) => {
        match $event {
            $(
                Event::$variant {
                    node_id,
                    endpoint_id,
                    attributes,
                } => attribute_changed(stringify!($variant), *node_id, *endpoint_id, attributes),
            )+
            Event::LightOn(node) => action("LightOn", node),
            Event::LightOff(node) => action("LightOff", node),
        }
    };
}

fn event_to_value(event: &Event) -> Value {
    project_events!(
        event,
        OnOffChanged,
        LevelControlChanged,
        ColorControlChanged,
        TemperatureMeasurementChanged,
        PressureMeasurementChanged,
        RelativeHumidityMeasurementChanged,
        OccupancySensingChanged,
        BooleanStateChanged,
        ThermostatChanged,
        FanControlChanged,
        DehumidificationControlChanged,
        ThermostatUserInterfaceConfigurationChanged,
        PowerSourceChanged,
        ElectricalPowerMeasurementChanged,
        ModeSelectChanged,
        MediaPlaybackChanged,
        MediaInputChanged,
        WindMeasurementChanged,
        CloudCoverChanged,
        DewPointChanged,
        UvIndexChanged,
        PrecipitationChanged,
        WeatherConditionChanged,
    )
}

/// One attribute-changed event: `{ node_id, endpoint_id, attributes }` inside
/// the single-argument variant the VM's field access unwraps, so
/// `event.attributes.on_off` resolves.
fn attribute_changed<T: serde::Serialize>(
    variant: &str,
    node_id: NodeId,
    endpoint_id: EndpointId,
    attributes: &T,
) -> Value {
    Value::Variant {
        enum_name: "Event".into(),
        variant: variant.into(),
        args: vec![Value::Struct(BTreeMap::from([
            ("node_id".into(), Value::Node(node_id)),
            ("endpoint_id".into(), Value::Int(i64::from(endpoint_id))),
            ("attributes".into(), serialized(attributes)),
        ]))],
    }
}

/// An action event, carrying the node it targets.
fn action(variant: &str, node: &Node) -> Value {
    Value::Variant {
        enum_name: "Event".into(),
        variant: variant.into(),
        args: vec![node_to_value(node)],
    }
}

/// Project `state` into the structure the deployment schema describes:
/// `{ <domain>: { <slug>: <node> } }`.
///
/// Driven by the schema rather than by the state, so the shape an automation
/// was checked against is the shape it is handed. A node the schema names but
/// the state no longer holds is left out, and the automation then fails on
/// the field access rather than acting on a node that has gone away.
fn state_to_value(state: &State, schema: &DeploymentSchema) -> Value {
    let mut domains = BTreeMap::new();
    for (domain, slugs) in &schema.domains {
        let mut nodes = BTreeMap::new();
        for (slug, node_id) in slugs {
            if let Some(node) = state.nodes.get(node_id) {
                nodes.insert(slug.clone(), node_to_value(node));
            }
        }
        domains.insert(domain.clone(), Value::Struct(nodes));
    }
    Value::Struct(domains)
}

/// Project one node.
///
/// `endpoints` is left out: `Value` has no map, and nothing in the language
/// reads a cluster off a node yet.
fn node_to_value(node: &Node) -> Value {
    Value::Struct(BTreeMap::from([
        ("id".into(), Value::Node(node.id)),
        ("entity_id".into(), Value::String(node.entity_id.clone())),
        (
            "integration".into(),
            Value::String(node.integration.clone()),
        ),
        (
            "name".into(),
            match &node.name {
                Some(name) => Value::String(name.clone()),
                None => Value::Unit,
            },
        ),
    ]))
}

/// Project a serialisable engine type into a [`Value`], through serde.
///
/// A stopgap, and deliberately a small one. Cluster snapshots are plain data
/// carrying the field names the language already uses, so serde reaches all
/// twenty-three of them in one function where writing each out by hand would
/// be several hundred lines that say nothing. It goes away when the clusters
/// can be walked by the `Facet` shapes the checker already types them from.
fn serialized<T: serde::Serialize>(value: &T) -> Value {
    match serde_json::to_value(value) {
        Ok(json) => json_to_value(json),
        // Unreachable for the cluster types, which are plain structs of
        // scalars: `to_value` fails only on a `Serialize` that errors or a
        // map with non-string keys.
        Err(e) => {
            tracing::error!("could not project an engine value: {}", e);
            Value::Unit
        }
    }
}

fn json_to_value(json: serde_json::Value) -> Value {
    match json {
        // An attribute a device has not reported reads as the unit value,
        // which is what an absent value is everywhere else in the language.
        serde_json::Value::Null => Value::Unit,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(n) => Value::Int(n),
            // A measurement that is not an integer, or one past `i64`; the
            // language's float is the closest it has either way.
            None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(items) => {
            Value::List(items.into_iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(fields) => Value::Struct(
            fields
                .into_iter()
                .map(|(name, value)| (name, json_to_value(value)))
                .collect(),
        ),
    }
}

// ============================================================================
// Action dispatch: body results into cluster commands
// ============================================================================

/// Decode a body's result into cluster commands.
///
/// Reported rather than asserted throughout. The checker types an observer
/// body as `[Event]`, but `Event` field access is still deferred, so a body
/// can type-check and return something else; a warning leaves the rest of the
/// deployment running where an assertion would take the daemon down.
fn dispatch_actions(sink: &dyn ActionSink, result: Value) {
    let Value::List(items) = result else {
        tracing::warn!("observer body returned {}, not a list of events", result);
        return;
    };
    for item in items {
        let Value::Variant {
            enum_name,
            variant,
            args,
        } = &item
        else {
            tracing::warn!("observer body returned {}, not an event", item);
            continue;
        };
        if enum_name != "Event" {
            tracing::warn!("observer body returned a {}, not an Event", enum_name);
            continue;
        }
        let command = match variant.as_str() {
            "LightOn" => ClusterCommand::OnOff(OnOffCommand::On),
            "LightOff" => ClusterCommand::OnOff(OnOffCommand::Off),
            // An attribute-changed variant is something the engine reports,
            // not something a body can ask for.
            other => {
                tracing::warn!("Event::{} is not an action", other);
                continue;
            }
        };
        match args.as_slice() {
            [Value::Struct(fields)] => match fields.get("id") {
                Some(Value::Node(node_id)) => {
                    sink.invoke_command(*node_id, ACTION_ENDPOINT, command)
                }
                _ => tracing::warn!("Event::{} names no node", variant),
            },
            _ => tracing::warn!("Event::{} takes one node", variant),
        }
    }
}
