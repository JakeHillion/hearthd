//! Tests for the runner.
//!
//! Each compiles real DSL source through the whole pipeline against a
//! deployment schema built from the same fake state the runner is dispatched
//! with, so what runs is the production path from source to cluster command.
//! Time is paused, which is what lets a five-minute `sleep_unique` be tested
//! in no wall time at all.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::automations::bytecode::BytecodeProgram;
use crate::matter::Endpoint;
use crate::matter::OccupancySensingCluster;
use crate::matter::OnOffCluster;

// ============================================================================
// Harness
// ============================================================================

/// An [`ActionSink`] that records what it is asked to send.
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

impl RecordingSink {
    /// The commands seen so far, rendered one per line.
    fn rendered(&self) -> String {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .map(|(node, endpoint, command)| format!("{} e{} {:?}", node, endpoint, command))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

const LAMP: u64 = 1;
const MOTION: u64 = 2;

fn node_id(raw: u64) -> NodeId {
    NodeId::from_raw(raw)
}

fn fake_node(raw: u64, entity_id: &str) -> Node {
    Node {
        id: node_id(raw),
        entity_id: entity_id.to_string(),
        integration: "test".to_string(),
        name: None,
        endpoints: HashMap::from([(ACTION_ENDPOINT, Endpoint::default())]),
    }
}

/// A lamp and a motion sensor, which is the smallest deployment the doc's
/// motion-light pair needs.
fn build_state() -> Arc<State> {
    let mut state = State::default();
    for (raw, entity_id) in [
        (LAMP, "light.living_room_lamp"),
        (MOTION, "binary_sensor.kitchen_motion"),
    ] {
        let node = fake_node(raw, entity_id);
        state.by_entity_id.insert(node.entity_id.clone(), node.id);
        state.nodes.insert(node.id, node);
    }
    Arc::new(state)
}

/// Compile one observer and relocate it against `schema`, asserting it type
/// checks cleanly and that the deployment has every entity it names.
fn compile_observer(source: &str, schema: &DeploymentSchema) -> CompiledAutomation {
    let program = crate::automations::parse(source).expect("source should parse");
    let lowered = crate::automations::desugar_program(program);
    let checked = crate::automations::check_program(&lowered);
    assert!(
        checked.errors.is_empty(),
        "source should type-check cleanly, got {:?}",
        checked.errors,
    );
    let hir = crate::automations::lower_program(&checked);
    let lir = crate::automations::lower_lir_program(&hir);
    let relocatable = crate::automations::lower_bytecode_program(&lir);
    let bytecode = crate::automations::relocate_program(&relocatable, schema)
        .expect("every entity the source names should be in the deployment");
    let auto = match bytecode {
        BytecodeProgram::Automation(auto) => auto,
        BytecodeProgram::Template { .. } => panic!("expected an Automation, got a Template"),
    };
    CompiledAutomation::new(
        auto.kind,
        auto.filter.expect("an observer with a filter compiles one"),
        auto.body,
    )
    .expect("the automation should build")
}

/// The observer that turns the lamp on the moment motion is reported.
const MOTION_ON: &str = "observer {
  event,
  state = { light = { living_room_lamp }, binary_sensor = { kitchen_motion }, ... },
  ...
} /event.node_id == kitchen_motion/ {
  [ Event::LightOn(living_room_lamp) ]
}";

/// The observer that turns it off five minutes after the last motion.
const MOTION_OFF: &str = "observer {
  event,
  state = { light = { living_room_lamp }, binary_sensor = { kitchen_motion }, ... },
  ...
} /event.node_id == kitchen_motion/ {
  if await sleep_unique(5min) { [ Event::LightOff(living_room_lamp) ] } else { [] }
}";

/// As `MOTION_OFF`, but saying out loud what the `else` branch is for: a
/// firing that was superseded turns the lamp back on instead.
///
/// Nothing sensible turns a lamp on when it is superseded — the point is
/// that an observable action in the `else` branch proves the body kept
/// running rather than being killed.
const MOTION_OFF_ELSE_ON: &str = "observer {
  event,
  state = { light = { living_room_lamp }, binary_sensor = { kitchen_motion }, ... },
  ...
} /event.node_id == kitchen_motion/ {
  if await sleep_unique(5min) {
    [ Event::LightOff(living_room_lamp) ]
  } else {
    [ Event::LightOn(living_room_lamp) ]
  }
}";

/// The same five-minute wait on a plain `sleep`, which nothing may cut short.
const MOTION_OFF_PLAIN_SLEEP: &str = "observer {
  event,
  state = { light = { living_room_lamp }, binary_sensor = { kitchen_motion }, ... },
  ...
} /event.node_id == kitchen_motion/ {
  await sleep(5min);
  [ Event::LightOff(living_room_lamp) ]
}";

/// A minute of uninterruptible `sleep` before the `sleep_unique`, which is
/// the case the two rules do not settle between them.
const MOTION_SLEEP_THEN_UNIQUE: &str = "observer {
  event,
  state = { light = { living_room_lamp }, binary_sensor = { kitchen_motion }, ... },
  ...
} /event.node_id == kitchen_motion/ {
  await sleep(1min);
  if await sleep_unique(5min) {
    [ Event::LightOff(living_room_lamp) ]
  } else {
    [ Event::LightOn(living_room_lamp) ]
  }
}";

fn motion() -> Event {
    Event::OccupancySensingChanged {
        node_id: node_id(MOTION),
        endpoint_id: ACTION_ENDPOINT,
        attributes: OccupancySensingCluster { occupancy: true },
    }
}

/// Let every spawned body run as far as it can right now.
///
/// A body needs one turn to reach its next suspension and another to act on
/// what woke it, and a test may have several of them in flight, so this
/// yields a few times rather than exactly twice. It cannot make a wait
/// elapse — only `tokio::time::advance` does that — so over-yielding never
/// hides a missing advance.
async fn settle() {
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}

// ============================================================================
// Dispatch
// ============================================================================

/// The whole path: an engine event reaches a filter, the filter passes, the
/// body runs, and the action it returns arrives as a cluster command
/// addressed to the node the automation named.
#[tokio::test(start_paused = true)]
async fn test_runner_motion_turns_the_light_on() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(
        sink.clone(),
        schema.clone(),
        vec![compile_observer(MOTION_ON, &schema)],
    );

    runner.dispatch(&motion(), &state);
    settle().await;

    insta::assert_snapshot!(sink.rendered(), @"1 e1 OnOff(On)");
}

/// An event the filter rejects fires nothing. The filter compares the event's
/// node against the motion sensor, so the lamp's own report must not loop
/// back into turning it on again.
#[tokio::test(start_paused = true)]
async fn test_runner_a_failing_filter_fires_nothing() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(
        sink.clone(),
        schema.clone(),
        vec![compile_observer(MOTION_ON, &schema)],
    );

    runner.dispatch(
        &Event::OnOffChanged {
            node_id: node_id(LAMP),
            endpoint_id: ACTION_ENDPOINT,
            attributes: OnOffCluster { on_off: true },
        },
        &state,
    );
    settle().await;

    insta::assert_snapshot!(sink.rendered(), @"");
}

/// A body that suspends acts when its wait elapses, not before.
#[tokio::test(start_paused = true)]
async fn test_runner_a_sleeping_body_acts_when_its_wait_elapses() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(
        sink.clone(),
        schema.clone(),
        vec![compile_observer(MOTION_OFF, &schema)],
    );

    runner.dispatch(&motion(), &state);
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @"");

    tokio::time::advance(Duration::from_secs(6 * 60)).await;
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @"1 e1 OnOff(Off)");
}

/// `sleep_unique`'s contract, end to end. Re-triggering inside the first
/// window abandons its body, so the light goes off five minutes after the
/// *last* motion rather than the first — and goes off once, not twice.
#[tokio::test(start_paused = true)]
async fn test_runner_re_triggering_restarts_the_window() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(
        sink.clone(),
        schema.clone(),
        vec![
            compile_observer(MOTION_ON, &schema),
            compile_observer(MOTION_OFF, &schema),
        ],
    );

    runner.dispatch(&motion(), &state);
    settle().await;

    // Four minutes in, motion again: the first off-body is abandoned and a
    // fresh five-minute window starts.
    tokio::time::advance(Duration::from_secs(4 * 60)).await;
    runner.dispatch(&motion(), &state);
    settle().await;

    // Eight minutes from the first trigger, past the first window but not
    // the second. Nothing has been turned off.
    tokio::time::advance(Duration::from_secs(4 * 60)).await;
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @r"
    1 e1 OnOff(On)
    1 e1 OnOff(On)
    ");

    // Past the second window.
    tokio::time::advance(Duration::from_secs(2 * 60)).await;
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @r"
    1 e1 OnOff(On)
    1 e1 OnOff(On)
    1 e1 OnOff(Off)
    ");
}

/// A superseded firing is not killed. Its `sleep_unique` resolves `false`
/// the moment a newer firing exists, and the body runs on into its `else`
/// branch — which is what `Future<Bool>` is for. Killing it instead would
/// make that branch unreachable.
#[tokio::test(start_paused = true)]
async fn test_runner_a_superseded_body_runs_its_else_branch() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(
        sink.clone(),
        schema.clone(),
        vec![compile_observer(MOTION_OFF_ELSE_ON, &schema)],
    );

    runner.dispatch(&motion(), &state);
    settle().await;

    // The second firing supersedes the first, which acts at once on the way
    // out rather than waiting or disappearing.
    tokio::time::advance(Duration::from_secs(60)).await;
    runner.dispatch(&motion(), &state);
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @"1 e1 OnOff(On)");

    // The second firing's own window then runs to its end.
    tokio::time::advance(Duration::from_secs(6 * 60)).await;
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @r"
    1 e1 OnOff(On)
    1 e1 OnOff(Off)
    ");
}

/// A plain `sleep` is never cut short. Two firings a minute apart both run
/// to their end, a minute apart, rather than the second replacing the first.
#[tokio::test(start_paused = true)]
async fn test_runner_a_plain_sleep_is_not_superseded() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(
        sink.clone(),
        schema.clone(),
        vec![compile_observer(MOTION_OFF_PLAIN_SLEEP, &schema)],
    );

    runner.dispatch(&motion(), &state);
    settle().await;
    tokio::time::advance(Duration::from_secs(60)).await;
    runner.dispatch(&motion(), &state);
    settle().await;

    // t+5min: the first firing's wait is out. The second's is not.
    tokio::time::advance(Duration::from_secs(4 * 60 + 1)).await;
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @"1 e1 OnOff(Off)");

    // t+6min: so is the second's. Both acted.
    tokio::time::advance(Duration::from_secs(60)).await;
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @r"
    1 e1 OnOff(Off)
    1 e1 OnOff(Off)
    ");
}

/// The case the two rules do not dictate between them, and the reading they
/// compose to: a firing superseded while it sat in a `sleep` serves that
/// `sleep` in full, then fails the `sleep_unique` it reaches afterwards
/// without waiting on it at all.
#[tokio::test(start_paused = true)]
async fn test_runner_a_sleep_survives_supersession_and_the_unique_after_it_does_not() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(
        sink.clone(),
        schema.clone(),
        vec![compile_observer(MOTION_SLEEP_THEN_UNIQUE, &schema)],
    );

    runner.dispatch(&motion(), &state);
    settle().await;

    // Superseded at t+30s, while the first firing is still inside its
    // one-minute `sleep`. Nothing happens yet: the `sleep` is untouchable.
    tokio::time::advance(Duration::from_secs(30)).await;
    runner.dispatch(&motion(), &state);
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @"");

    // t+60s: the first firing's `sleep` ends and it reaches its
    // `sleep_unique`, which fails immediately — the newer firing already
    // exists — so it takes the `else` branch without waiting five minutes.
    tokio::time::advance(Duration::from_secs(30)).await;
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @"1 e1 OnOff(On)");

    // The second firing is untouched: its own `sleep` ends at t+90s, and the
    // `sleep_unique` after it runs the full five minutes from there, because
    // nothing superseded it. Advanced one deadline at a time, so each wait
    // resolves in its own step.
    tokio::time::advance(Duration::from_secs(35)).await;
    settle().await;
    tokio::time::advance(Duration::from_secs(5 * 60)).await;
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @r"
    1 e1 OnOff(On)
    1 e1 OnOff(Off)
    ");
}

// ============================================================================
// Projection
// ============================================================================

/// An attribute-changed event projects into the shape field access unwraps,
/// so `event.attributes.on_off` and `event.node_id` both resolve.
#[test]
fn test_runner_event_projects_its_attributes() {
    let event = Event::OnOffChanged {
        node_id: node_id(7),
        endpoint_id: 1,
        attributes: OnOffCluster { on_off: true },
    };
    insta::assert_snapshot!(
        event_to_value(&event),
        @"Event::OnOffChanged({attributes: {on_off: true}, endpoint_id: 1, node_id: node(7)})"
    );
}

/// An attribute a device has not reported projects as the unit value rather
/// than being absent, so reading it is a value and not a failure.
#[test]
fn test_runner_event_projects_an_unreported_attribute_as_unit() {
    let event = Event::LevelControlChanged {
        node_id: node_id(7),
        endpoint_id: 1,
        attributes: crate::matter::LevelControlCluster {
            current_level: None,
        },
    };
    insta::assert_snapshot!(
        event_to_value(&event),
        @"Event::LevelControlChanged({attributes: {current_level: ()}, endpoint_id: 1, node_id: node(7)})"
    );
}

/// An action event carries the node itself, which is what lets the runner
/// address a command without a reverse lookup.
#[test]
fn test_runner_action_event_carries_its_node() {
    insta::assert_snapshot!(
        event_to_value(&Event::LightOn(fake_node(LAMP, "light.living_room_lamp"))),
        @"Event::LightOn(node(1))"
    );
}

/// `state` takes the shape the schema describes, which is the shape the
/// checker typed the automation against.
#[test]
fn test_runner_state_projects_the_schema_shape() {
    let state = build_state();
    let schema = DeploymentSchema::from_state(&state);
    insta::assert_snapshot!(
        state_to_value(&state, &schema),
        @"{binary_sensor: {kitchen_motion: node(2)}, climate: {}, light: {living_room_lamp: node(1)}, media_player: {}, sensor: {}, speaker: {}, weather: {}}"
    );
}

/// A node the schema names but the state no longer holds is left out, rather
/// than projected from a stale copy.
#[test]
fn test_runner_state_omits_a_departed_node() {
    let state = build_state();
    let schema = DeploymentSchema::from_state(&state);
    let mut without_lamp = State::clone(&state);
    without_lamp.nodes.remove(&node_id(LAMP));
    insta::assert_snapshot!(
        state_to_value(&without_lamp, &schema),
        @"{binary_sensor: {kitchen_motion: node(2)}, climate: {}, light: {}, media_player: {}, sensor: {}, speaker: {}, weather: {}}"
    );
}

// ============================================================================
// Action decoding
// ============================================================================

/// A body result that is not a list of events is reported and dropped.
/// `Event` field access still types as `Error`, so a body can check and
/// return something else, and the daemon must survive it.
#[test]
fn test_runner_a_non_list_body_result_sends_nothing() {
    let sink = RecordingSink::default();
    dispatch_actions(&sink, Value::Bool(true));
    dispatch_actions(&sink, Value::List(vec![Value::Int(1)]));
    dispatch_actions(
        &sink,
        Value::List(vec![Value::Variant {
            enum_name: "Event".into(),
            variant: "OnOffChanged".into(),
            args: vec![Value::Struct(BTreeMap::new())],
        }]),
    );
    insta::assert_snapshot!(sink.rendered(), @"");
}

// ============================================================================
// The shipped examples
// ============================================================================

// The automations under `examples/automations/` are what a new deployment is
// pointed at, so they are compiled here rather than described. `include_str!`
// is what keeps the files themselves checked: a copy of their text would
// drift from the files the moment either changed.
const EXAMPLE_MOTION_ON: &str =
    include_str!("../../../../../examples/automations/kitchen_motion_on.hda");
const EXAMPLE_MOTION_OFF: &str =
    include_str!("../../../../../examples/automations/kitchen_motion_off.hda");

/// The shipped pair, run against the deployment they are written for: motion
/// turns the lamp on at once and off five minutes later.
#[tokio::test(start_paused = true)]
async fn test_runner_the_shipped_examples_drive_a_light() {
    let state = build_state();
    let schema = Arc::new(DeploymentSchema::from_state(&state));
    let sink = Arc::new(RecordingSink::default());
    let runner = Runner::new(
        sink.clone(),
        schema.clone(),
        vec![
            compile_observer(EXAMPLE_MOTION_ON, &schema),
            compile_observer(EXAMPLE_MOTION_OFF, &schema),
        ],
    );

    runner.dispatch(&motion(), &state);
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @"1 e1 OnOff(On)");

    tokio::time::advance(Duration::from_secs(6 * 60)).await;
    settle().await;
    insta::assert_snapshot!(sink.rendered(), @r"
    1 e1 OnOff(On)
    1 e1 OnOff(Off)
    ");
}
