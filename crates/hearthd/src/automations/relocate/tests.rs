//! Tests for the relocation pass.
//!
//! Each test compiles real DSL source the whole way down and relocates the
//! result against a deployment built by hand, so what is exercised is the
//! same artifact the runner would relocate.

use std::collections::HashMap;

use super::BytecodeProgram;
use super::relocate_program;
use crate::automations::bytecode::Const;
use crate::automations::bytecode::RelocatableProgram;
use crate::automations::domain::Domain;
use crate::automations::entity_index::EntityIndex;
use crate::automations::pretty_print::PrettyPrint;
use crate::engine::NodeId;
use crate::engine::state::State;

/// The entities a deployment has, keyed by whole `entity_id` so a fixture
/// reads as the names a house answers to.
struct Deployment(HashMap<String, NodeId>);

impl EntityIndex for Deployment {
    fn resolve(&self, domain: Domain, slug: &str) -> Option<NodeId> {
        self.0.get(&format!("{}.{}", domain, slug)).copied()
    }
}

fn deployment(entries: &[(&str, u64)]) -> Deployment {
    Deployment(
        entries
            .iter()
            .map(|(entity_id, raw)| (entity_id.to_string(), NodeId::from_raw(*raw)))
            .collect(),
    )
}

/// A deployment with no devices at all.
fn empty() -> Deployment {
    Deployment(HashMap::new())
}

/// Compile source to the relocatable form the compiler stops at.
fn compile(input: &str) -> RelocatableProgram {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    let checked = crate::automations::check_program(&lowered);
    assert!(checked.errors.is_empty(), "{:?}", checked.errors);
    let hir = crate::automations::lower_program(&checked);
    let lir = crate::automations::lower_lir_program(&hir);
    crate::automations::lower_bytecode_program(&lir)
}

/// The constant pools of every function in a relocated program.
fn pools(program: &BytecodeProgram) -> Vec<Vec<Const>> {
    let autos = match program {
        BytecodeProgram::Automation(auto) => std::slice::from_ref(auto),
        BytecodeProgram::Template { automations, .. } => automations.as_slice(),
    };
    autos
        .iter()
        .flat_map(|auto| auto.filter.iter().chain(std::iter::once(&auto.body)))
        .map(|f| f.consts.clone())
        .collect()
}

const NAMES_A_LAMP: &str =
    r#"observer { event, state, ... } /state.light.living_room_lamp.entity_id == "x"/ { [event] }"#;

/// A named entity the deployment has becomes the node it stands for.
#[test]
fn test_symbol_resolves_to_the_node_it_names() {
    let program = compile(NAMES_A_LAMP);
    let relocated = relocate_program(&program, &deployment(&[("light.living_room_lamp", 7)]))
        .expect("the deployment has the lamp");

    let resolved: Vec<_> = pools(&relocated)
        .into_iter()
        .flatten()
        .filter(|c| matches!(c, Const::Node(_)))
        .collect();
    assert_eq!(resolved, vec![Const::Node(NodeId::from_raw(7))]);
}

/// The same compiled program relocated against a deployment without the
/// lamp. This is the case the whole design exists for: the failure is in
/// relocation, and the compiled program is untouched and still usable.
#[test]
fn test_missing_entity_fails_relocation_not_compilation() {
    let program = compile(NAMES_A_LAMP);
    let errors = relocate_program(&program, &deployment(&[("light.kitchen", 1)]))
        .expect_err("the deployment has no living room lamp");

    let rendered: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    insta::assert_debug_snapshot!(rendered, @r#"
    [
        "no entity 'light.living_room_lamp' in this deployment",
    ]
    "#);

    // The failure consumed nothing: the same artifact relocates against a
    // deployment that does have the lamp, with no recompilation.
    assert!(relocate_program(&program, &deployment(&[("light.living_room_lamp", 3)])).is_ok());
}

/// A domain the deployment has nothing in is not a failure — only a named
/// entity can fail to resolve, and this names none.
#[test]
fn test_domain_group_alone_relocates_against_an_empty_deployment() {
    let program =
        compile(r#"observer { event, state, ... } /true/ { let lights = state.light; [event] }"#);
    assert!(relocate_program(&program, &empty()).is_ok());
}

/// Every unresolved name is reported, not just the first, so one relocation
/// tells you everything the deployment is missing.
#[test]
fn test_every_missing_entity_is_reported() {
    let program = compile(
        r#"observer { event, state, ... } /state.light.a.entity_id == state.light.b.entity_id/ { [event] }"#,
    );
    let errors = relocate_program(&program, &empty()).expect_err("the deployment has neither");

    let rendered: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    insta::assert_debug_snapshot!(rendered, @r#"
    [
        "no entity 'light.a' in this deployment",
        "no entity 'light.b' in this deployment",
    ]
    "#);
}

/// Relocation rewrites the constant pool and nothing else. The bytes a
/// deployment executes are the bytes the compiler emitted.
#[test]
fn test_relocation_leaves_the_code_stream_alone() {
    let program = compile(NAMES_A_LAMP);
    let relocated = relocate_program(&program, &deployment(&[("light.living_room_lamp", 7)]))
        .expect("the deployment has the lamp");

    let (RelocatableProgram::Automation(before), BytecodeProgram::Automation(after)) =
        (&program, &relocated)
    else {
        panic!("expected an Automation on both sides");
    };
    assert_eq!(
        before.filter.as_ref().map(|f| &f.code),
        after.filter.as_ref().map(|f| &f.code)
    );
    assert_eq!(before.body.code, after.body.code);
}

/// The engine's own index answers relocation. `State` maintains
/// `by_entity_id` so the API can resolve a user-facing name; relocation asks
/// that same index, so a linked automation and an API call cannot disagree
/// about which node a name means.
#[test]
fn test_relocates_against_engine_state() {
    let mut state = State::default();
    state
        .by_entity_id
        .insert("light.living_room_lamp".to_string(), NodeId::from_raw(11));

    let program = compile(NAMES_A_LAMP);
    let relocated =
        relocate_program(&program, &state).expect("the engine knows the living room lamp");

    let resolved: Vec<_> = pools(&relocated)
        .into_iter()
        .flatten()
        .filter(|c| matches!(c, Const::Node(_)))
        .collect();
    assert_eq!(resolved, vec![Const::Node(NodeId::from_raw(11))]);
}

// =============================================================================
// Disassembly
//
// Relocation is visible in the listing: a symbol slot becomes a node, and
// nothing else about the function moves.
// =============================================================================

/// The listing before and after relocation, which is the whole claim the pass
/// makes in one place: `#0` is filled in, and every instruction, register and
/// parameter is exactly where it was.
#[test]
fn test_relocated_automation_disassembles() {
    let program = compile(NAMES_A_LAMP);
    insta::assert_snapshot!(program.to_pretty_string(), @r#"
    Automation: observer
      filter:
        regs: 6
        params:
          r0: event [Event]
          r1: state [State]
        consts:
          #0 = entity light.living_room_lamp
          #1 = ident entity_id
          #2 = string "x"
        code:
          load_const_node    r2, #0 (entity light.living_room_lamp)
          field              r3, r2, #1 (entity_id)
          load_const_string  r4, #2 ("x")
          eq                 r5, r3, r4
          return             r5
      body:
        regs: 3
        params:
          r0: event [Event]
          r1: state [State]
        code:
          list               r2, [r0]
          return             r2
    "#);

    let relocated = relocate_program(&program, &deployment(&[("light.living_room_lamp", 7)]))
        .expect("the deployment has the lamp");
    insta::assert_snapshot!(relocated.to_pretty_string(), @r#"
    Automation: observer
      filter:
        regs: 6
        params:
          r0: event [Event]
          r1: state [State]
        consts:
          #0 = node 7
          #1 = ident entity_id
          #2 = string "x"
        code:
          load_const_node    r2, #0 (node 7)
          field              r3, r2, #1 (entity_id)
          load_const_string  r4, #2 ("x")
          eq                 r5, r3, r4
          return             r5
      body:
        regs: 3
        params:
          r0: event [Event]
          r1: state [State]
        code:
          list               r2, [r0]
          return             r2
    "#);
}

/// A template relocates every automation it holds, and prints as one.
#[test]
fn test_relocated_template_disassembles() {
    let program = compile(
        r#"{ room: String }: [
            observer { event, state, ... } /state.light.a.entity_id == "x"/ { [event] },
            observer { event, state, ... } /state.climate.b.entity_id == "y"/ { [event] }
        ]"#,
    );
    let relocated = relocate_program(&program, &deployment(&[("light.a", 4), ("climate.b", 9)]))
        .expect("the deployment has both");
    insta::assert_snapshot!(relocated.to_pretty_string(), @r#"
    Template:
      Params:
        Param: room
          Type::Named: String
      Automations:
        Automation: observer
          filter:
            regs: 6
            params:
              r0: event [Event]
              r1: state [State]
            consts:
              #0 = node 4
              #1 = ident entity_id
              #2 = string "x"
            code:
              load_const_node    r2, #0 (node 4)
              field              r3, r2, #1 (entity_id)
              load_const_string  r4, #2 ("x")
              eq                 r5, r3, r4
              return             r5
          body:
            regs: 3
            params:
              r0: event [Event]
              r1: state [State]
            code:
              list               r2, [r0]
              return             r2
        Automation: observer
          filter:
            regs: 6
            params:
              r0: event [Event]
              r1: state [State]
            consts:
              #0 = node 9
              #1 = ident entity_id
              #2 = string "y"
            code:
              load_const_node    r2, #0 (node 9)
              field              r3, r2, #1 (entity_id)
              load_const_string  r4, #2 ("y")
              eq                 r5, r3, r4
              return             r5
          body:
            regs: 3
            params:
              r0: event [Event]
              r1: state [State]
            code:
              list               r2, [r0]
              return             r2
    "#);
}
