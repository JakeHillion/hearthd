//! End-to-end test: drive the actual on-disk example automation files
//! through every pipeline stage, dispatch motion events, and assert the
//! resulting cluster command sequence.
//!
//! This is the smallest fully wired motion-sensor-to-light flow we
//! support: state is hand-built (no MQTT broker), the engine never
//! runs its event loop (we invoke the runner directly), but every
//! other stage — parse, desugar, check, lower, lower_lir,
//! lower_bytecode, the bytecode VM, and the `Runner` — is the real
//! production code path.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use hearthd::AutomationRunner;
use hearthd::Event;
use hearthd::State;
use hearthd::automations::repr::ast::AutomationKind;
use hearthd::automations::runtime::ActionSink;
use hearthd::automations::runtime::CompiledAutomation;
use hearthd::automations::runtime::Runner;
use hearthd::automations::schema::DeploymentSchema;
use hearthd::matter::ClusterCommand;
use hearthd::matter::Endpoint;
use hearthd::matter::EndpointId;
use hearthd::matter::Node;
use hearthd::matter::NodeId;
use hearthd::matter::OccupancySensingCluster;
use hearthd::matter::OnOffCommand;

// The on-disk copies of these automations live in
// `examples/automations/kitchen_motion_{on,off}.hda` for documentation;
// the test embeds them as string literals so the nix build sandbox
// (which only includes `.rs` files via crane's `cleanCargoSource`) can
// still compile against them.
const KITCHEN_MOTION_ON: &str = r#"observer {
  event,
  state = {
    light = { living_room_lamp },
    binary_sensor = { kitchen_motion },
    ...
  },
  ...
} /event.node_id == kitchen_motion.id/ {
  [ Event::LightOn(living_room_lamp) ]
}"#;

const KITCHEN_MOTION_OFF: &str = r#"observer {
  event,
  state = {
    light = { living_room_lamp },
    binary_sensor = { kitchen_motion },
    ...
  },
  ...
} /event.node_id == kitchen_motion.id/ {
  if await sleep_unique(5min) {
    [ Event::LightOff(living_room_lamp) ]
  } else {
    []
  }
}"#;

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

fn build_state() -> Arc<State> {
    let mut state = State::default();
    for (id, entity_id) in [
        (1u64, "light.living_room_lamp"),
        (2u64, "binary_sensor.kitchen_motion"),
    ] {
        state.nodes.insert(id, fake_node(id, entity_id));
        state.by_entity_id.insert(entity_id.to_string(), id);
    }
    Arc::new(state)
}

fn compile(id: usize, source: &str, schema: Arc<DeploymentSchema>) -> CompiledAutomation {
    let program = hearthd::automations::parse(source).expect("parse");
    let lowered = hearthd::automations::desugar_program(program);
    let typed = hearthd::automations::check::check_program_with_schema(&lowered, schema);
    assert!(
        typed.errors.is_empty(),
        "type errors in example:\n{:?}",
        typed.errors
    );
    let hir = hearthd::automations::lower_program(&typed);
    let lir = hearthd::automations::lower_lir_program(&hir);
    let bc = hearthd::automations::lower_bytecode_program(&lir);
    let auto = match bc {
        hearthd::automations::repr::BytecodeProgram::Automation(a) => a,
        _ => panic!("expected an Automation, got a Template"),
    };
    CompiledAutomation {
        id,
        kind: AutomationKind::Observer,
        filter: auto.filter.expect("filter present"),
        body: auto.body,
    }
}

#[tokio::test(start_paused = true)]
async fn motion_sensor_drives_light_on_then_off() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let on_auto = compile(0, KITCHEN_MOTION_ON, schema.clone());
    let off_auto = compile(1, KITCHEN_MOTION_OFF, schema.clone());

    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(sink.clone(), schema, vec![on_auto, off_auto]);

    let motion = Event::OccupancySensingChanged {
        node_id: 2,
        endpoint_id: 1,
        attributes: OccupancySensingCluster { occupancy: true },
    };

    // Fire motion once. The on-observer should emit Light On immediately;
    // the off-observer queues a 5-minute sleep.
    runner.dispatch(&motion, &state);
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    {
        let cmds = sink.commands.lock().unwrap();
        assert_eq!(cmds.len(), 1, "exactly one immediate command");
        assert!(matches!(cmds[0].2, ClusterCommand::OnOff(OnOffCommand::On)));
        assert_eq!(cmds[0].0, 1); // living_room_lamp
    }

    // Advance past the 5-minute window — Off must fire.
    tokio::time::advance(Duration::from_secs(6 * 60)).await;
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    let cmds = sink.commands.lock().unwrap();
    assert_eq!(cmds.len(), 2, "On + Off after the window elapses");
    assert!(matches!(
        cmds[1].2,
        ClusterCommand::OnOff(OnOffCommand::Off)
    ));
    assert_eq!(cmds[1].0, 1); // living_room_lamp
}

#[tokio::test(start_paused = true)]
async fn motion_retrigger_keeps_light_on_through_first_window() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let on_auto = compile(0, KITCHEN_MOTION_ON, schema.clone());
    let off_auto = compile(1, KITCHEN_MOTION_OFF, schema.clone());

    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(sink.clone(), schema, vec![on_auto, off_auto]);

    let motion = Event::OccupancySensingChanged {
        node_id: 2,
        endpoint_id: 1,
        attributes: OccupancySensingCluster { occupancy: true },
    };

    // First motion: starts a 5min sleep.
    runner.dispatch(&motion, &state);
    tokio::task::yield_now().await;

    // 4 minutes in — re-trigger. The prior off task is aborted; the
    // second instance begins a fresh 5min window.
    tokio::time::advance(Duration::from_secs(4 * 60)).await;
    runner.dispatch(&motion, &state);
    tokio::task::yield_now().await;

    // 4 more minutes (8 total) — still inside the *second* window.
    tokio::time::advance(Duration::from_secs(4 * 60)).await;
    tokio::task::yield_now().await;
    {
        let cmds = sink.commands.lock().unwrap();
        let offs = cmds
            .iter()
            .filter(|(_, _, c)| matches!(c, ClusterCommand::OnOff(OnOffCommand::Off)))
            .count();
        assert_eq!(offs, 0, "no Off before the second window elapses");
    }

    // Push past the second window — Off must fire exactly once.
    tokio::time::advance(Duration::from_secs(2 * 60)).await;
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    let cmds = sink.commands.lock().unwrap();
    let offs: Vec<_> = cmds
        .iter()
        .filter(|(_, _, c)| matches!(c, ClusterCommand::OnOff(OnOffCommand::Off)))
        .collect();
    assert_eq!(offs.len(), 1, "exactly one Off after the second window");
}
