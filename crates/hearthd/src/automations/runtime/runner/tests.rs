use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::automations::repr::ast::AutomationKind;
use crate::automations::schema::DeploymentSchema;
use crate::engine::AutomationRunner;
use crate::engine::Event;
use crate::engine::State;
use crate::matter::ClusterCommand;
use crate::matter::Endpoint;
use crate::matter::EndpointId;
use crate::matter::Node;
use crate::matter::NodeId;
use crate::matter::OccupancySensingCluster;
use crate::matter::OnOffCommand;

/// Recording sink: stores every cluster command it sees.
#[derive(Default)]
struct RecordingSink {
    commands: Mutex<Vec<(NodeId, EndpointId, ClusterCommand)>>,
}

impl ActionSink for RecordingSink {
    fn invoke_command(&self, node_id: NodeId, endpoint_id: EndpointId, command: ClusterCommand) {
        self.commands
            .lock()
            .unwrap()
            .push((node_id, endpoint_id, command));
    }
}

fn fake_node(id: NodeId, entity_id: &str) -> Node {
    Node {
        id,
        entity_id: entity_id.to_string(),
        integration: "test".to_string(),
        name: None,
        endpoints: HashMap::from([(1u16, Endpoint::default())]),
    }
}

fn build_state(entries: &[(&str, NodeId)]) -> Arc<State> {
    let mut state = State::default();
    for (entity_id, id) in entries {
        state.nodes.insert(*id, fake_node(*id, entity_id));
        state.by_entity_id.insert(entity_id.to_string(), *id);
    }
    Arc::new(state)
}

fn build_schema(state: &State) -> Arc<DeploymentSchema> {
    Arc::new(DeploymentSchema::from_state(state))
}

/// Compile an observer source string all the way to bytecode and wrap
/// it as a `CompiledAutomation` with the given runner-assigned id.
fn compile_observer(
    id: AutomationId,
    source: &str,
    schema: Arc<DeploymentSchema>,
) -> CompiledAutomation {
    let program = crate::automations::parse(source).expect("parse");
    let lowered = crate::automations::desugar_program(program);
    let typed = crate::automations::check::check_program_with_schema(&lowered, schema);
    assert!(typed.errors.is_empty(), "type errors: {:?}", typed.errors);
    let hir = crate::automations::lower_program(&typed);
    let lir = crate::automations::lower_lir_program(&hir);
    let bc = crate::automations::lower_bytecode_program(&lir);
    let auto = match bc {
        crate::automations::repr::BytecodeProgram::Automation(a) => *Box::new(a),
        _ => panic!("expected an Automation"),
    };
    CompiledAutomation {
        id,
        kind: AutomationKind::Observer,
        filter: auto.filter.expect("filter present"),
        body: auto.body,
    }
}

#[tokio::test(start_paused = true)]
async fn motion_triggers_light_on_immediately() {
    // Observer that turns the lamp on whenever the kitchen motion
    // sensor reports occupancy.
    let state = build_state(&[
        ("light.living_room_lamp", 1),
        ("binary_sensor.kitchen_motion", 2),
    ]);
    let schema = build_schema(&state);

    let auto = compile_observer(
        0,
        "observer { event, state = { light = { living_room_lamp }, binary_sensor = { kitchen_motion }, ... }, ... } /event.node_id == 2/ { [ Event::LightOn(living_room_lamp) ] }",
        schema.clone(),
    );

    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(sink.clone(), schema, vec![auto]);

    runner.dispatch(
        &Event::OccupancySensingChanged {
            node_id: 2,
            endpoint_id: 1,
            attributes: OccupancySensingCluster { occupancy: true },
        },
        &state,
    );

    // The body runs in a spawned task; yield so it can complete.
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    let commands = sink.commands.lock().unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0, 1); // living_room_lamp
    assert_eq!(commands[0].1, 1); // endpoint 1
    assert!(matches!(
        commands[0].2,
        ClusterCommand::OnOff(OnOffCommand::On)
    ));
}

#[tokio::test(start_paused = true)]
async fn re_trigger_cancels_pending_off_timer() {
    // Two observers form the doc's motion-light pair: one turns the
    // light on immediately, the other waits `sleep_unique(5min)` and
    // turns it off. A re-trigger during the wait must cancel the
    // pending off task — only the second trigger's 5min window ever
    // resolves to an Off command.
    let state = build_state(&[
        ("light.living_room_lamp", 1),
        ("binary_sensor.kitchen_motion", 2),
    ]);
    let schema = build_schema(&state);

    let on_observer = compile_observer(
        0,
        "observer { event, state = { light = { living_room_lamp }, binary_sensor = { kitchen_motion }, ... }, ... } /event.node_id == 2/ { [ Event::LightOn(living_room_lamp) ] }",
        schema.clone(),
    );
    let off_observer = compile_observer(
        1,
        "observer { event, state = { light = { living_room_lamp }, binary_sensor = { kitchen_motion }, ... }, ... } /event.node_id == 2/ { if await sleep_unique(5min) { [ Event::LightOff(living_room_lamp) ] } else { [] } }",
        schema.clone(),
    );

    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(sink.clone(), schema, vec![on_observer, off_observer]);

    // First motion: on fires immediately, off is queued for +5min.
    runner.dispatch(
        &Event::OccupancySensingChanged {
            node_id: 2,
            endpoint_id: 1,
            attributes: OccupancySensingCluster { occupancy: true },
        },
        &state,
    );
    tokio::task::yield_now().await;

    // Advance 4 minutes — still inside the first sleep window.
    tokio::time::advance(Duration::from_secs(4 * 60)).await;

    // Re-trigger before the first 5min window elapses. The prior off
    // task is aborted; the second 5min window starts.
    runner.dispatch(
        &Event::OccupancySensingChanged {
            node_id: 2,
            endpoint_id: 1,
            attributes: OccupancySensingCluster { occupancy: true },
        },
        &state,
    );
    tokio::task::yield_now().await;

    // After another 4 minutes (8 total) the second window hasn't
    // elapsed either; no Off should have fired yet.
    tokio::time::advance(Duration::from_secs(4 * 60)).await;
    tokio::task::yield_now().await;

    {
        let commands = sink.commands.lock().unwrap();
        let offs = commands
            .iter()
            .filter(|(_, _, c)| matches!(c, ClusterCommand::OnOff(OnOffCommand::Off)))
            .count();
        assert_eq!(
            offs, 0,
            "no Off should fire before the second window elapses"
        );
    }

    // Advance past the second window — the second instance fires Off.
    tokio::time::advance(Duration::from_secs(2 * 60)).await;
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    let commands = sink.commands.lock().unwrap();
    let offs: Vec<_> = commands
        .iter()
        .filter(|(_, _, c)| matches!(c, ClusterCommand::OnOff(OnOffCommand::Off)))
        .collect();
    assert_eq!(offs.len(), 1, "exactly one Off should fire");
    assert_eq!(offs[0].0, 1);
}

/// The runner gracefully ignores body output that isn't a list.
#[test]
fn dispatch_actions_non_list_is_warning_not_panic() {
    let sink = RecordingSink::default();
    super::dispatch_actions(&sink, Value::Bool(true));
    assert!(sink.commands.lock().unwrap().is_empty());
}

/// Plain field projections of an `OnOffCluster` event resolve correctly
/// through the synthesised value layer.
#[test]
fn event_to_value_exposes_attribute_fields() {
    let event = Event::OnOffChanged {
        node_id: 7,
        endpoint_id: 1,
        attributes: crate::matter::OnOffCluster { on_off: true },
    };
    let v = super::event_to_value(&event);
    // event.attributes.on_off
    let Value::Variant { args, .. } = &v else {
        panic!("expected variant");
    };
    let Value::Struct(outer) = &args[0] else {
        panic!("expected struct");
    };
    let Value::Struct(attrs) = outer.get("attributes").expect("attributes") else {
        panic!("expected attributes struct");
    };
    assert_eq!(attrs.get("on_off"), Some(&Value::Bool(true)));
}

/// Building a state value with a schema produces the structural layout
/// observer destructuring expects.
#[test]
fn state_to_value_uses_schema_domains() {
    let state = build_state(&[
        ("light.living_room_lamp", 1),
        ("binary_sensor.kitchen_motion", 2),
    ]);
    let schema = build_schema(&state);
    let v = super::state_to_value(&state, &schema);
    let Value::Struct(domains) = &v else {
        panic!("expected struct");
    };
    let Value::Struct(lights) = domains.get("light").expect("light domain") else {
        panic!("expected struct");
    };
    let Value::Struct(node) = lights.get("living_room_lamp").expect("node") else {
        panic!("expected node struct");
    };
    assert_eq!(node.get("id"), Some(&Value::Int(1)));
    assert_eq!(
        node.get("entity_id"),
        Some(&Value::String("light.living_room_lamp".into()))
    );
    let _ = BTreeMap::<String, Value>::new();
}
