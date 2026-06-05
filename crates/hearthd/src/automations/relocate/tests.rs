//! Tests for the relocation pass.
//!
//! Each test compiles real DSL source the whole way down and relocates the
//! result against a deployment built by hand, so what is exercised is the
//! same artifact the runner would relocate.

use std::collections::HashMap;

use super::relocate_program;
use crate::automations::repr::bytecode::BytecodeProgram;
use crate::automations::repr::bytecode::Const;
use crate::automations::repr::bytecode::RelocatableProgram;
use crate::automations::schema::DeploymentSchema;
use crate::engine::NodeId;
use crate::engine::state::State;
use crate::matter::Node;

/// A deployment with one node per `(entity_id, node id)` entry.
fn schema(entries: &[(&str, u64)]) -> DeploymentSchema {
    let mut state = State::default();
    for (entity_id, raw) in entries {
        state.nodes.insert(
            NodeId::from_raw(*raw),
            Node {
                entity_id: entity_id.to_string(),
                integration: "test".to_string(),
                name: None,
                endpoints: HashMap::new(),
            },
        );
    }
    DeploymentSchema::from_state(&state)
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
    let relocated = relocate_program(&program, &schema(&[("light.living_room_lamp", 7)]))
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
    let errors = relocate_program(&program, &schema(&[("light.kitchen", 1)]))
        .expect_err("the deployment has no living room lamp");

    let rendered: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    insta::assert_debug_snapshot!(rendered, @r#"
    [
        "no entity 'light.living_room_lamp' in this deployment",
    ]
    "#);

    // The failure consumed nothing: the same artifact relocates against a
    // deployment that does have the lamp, with no recompilation.
    assert!(relocate_program(&program, &schema(&[("light.living_room_lamp", 3)])).is_ok());
}

/// A domain the deployment has nothing in is not a failure — only a named
/// entity can fail to resolve, and this names none.
#[test]
fn test_domain_group_alone_relocates_against_an_empty_deployment() {
    let program =
        compile(r#"observer { event, state, ... } /true/ { let lights = state.light; [event] }"#);
    assert!(relocate_program(&program, &DeploymentSchema::default()).is_ok());
}

/// Every unresolved name is reported, not just the first, so one relocation
/// tells you everything the deployment is missing.
#[test]
fn test_every_missing_entity_is_reported() {
    let program = compile(
        r#"observer { event, state, ... } /state.light.a.entity_id == state.light.b.entity_id/ { [event] }"#,
    );
    let errors = relocate_program(&program, &DeploymentSchema::default())
        .expect_err("the deployment has neither");

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
    let relocated = relocate_program(&program, &schema(&[("light.living_room_lamp", 7)]))
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
