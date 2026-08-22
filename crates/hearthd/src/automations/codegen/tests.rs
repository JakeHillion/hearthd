use crate::automations::repr::pretty_print::PrettyPrint;

/// Run the full pipeline: parse → desugar → check → lower → codegen,
/// then pretty-print the input, HIR, and LIR together.
fn codegen_and_pretty(input: &str) -> String {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    let result = crate::automations::check_program(&lowered);
    let hir = crate::automations::lower_program(&result);
    let lir = crate::automations::codegen_program(&hir).expect("codegen should succeed");
    format!(
        "Input:\n{}\n\n---\nHIR:\n{}\n---\nLIR:\n{}",
        input,
        hir.to_pretty_string(),
        lir.to_pretty_string()
    )
}

// =============================================================================
// Simple constants
// =============================================================================

#[test]
fn test_codegen_constants() {
    let result = codegen_and_pretty("observer {} /true/ { 42; 1.5; \"hello\"; false; [] }");
    insta::assert_snapshot!(result);
}

#[test]
fn test_codegen_unit_literals() {
    let result = codegen_and_pretty("observer {} /true/ { 5s; 30min; 25c; 90deg; [] }");
    insta::assert_snapshot!(result);
}

// =============================================================================
// Binary/unary operations
// =============================================================================

#[test]
fn test_codegen_binary_ops() {
    let result = codegen_and_pretty("observer {} /true/ { 1 + 2 * 3; [] }");
    insta::assert_snapshot!(result);
}

#[test]
fn test_codegen_unary_ops() {
    let result = codegen_and_pretty("observer {} /true/ { let x = 10; -x; !true; [] }");
    insta::assert_snapshot!(result);
}

// =============================================================================
// Control flow
// =============================================================================

#[test]
fn test_codegen_if_else() {
    let result = codegen_and_pretty("observer {} /true/ { if true { [] } else { [] } }");
    insta::assert_snapshot!(result);
}

#[test]
fn test_codegen_short_circuit_and() {
    let result = codegen_and_pretty("observer {} /true && false/ { [] }");
    insta::assert_snapshot!(result);
}

#[test]
fn test_codegen_short_circuit_or() {
    let result = codegen_and_pretty("observer {} /true || false/ { [] }");
    insta::assert_snapshot!(result);
}

// =============================================================================
// List comprehension (for loop + iterator)
// =============================================================================

#[test]
fn test_codegen_list_comprehension() {
    let src = r#"observer {
  state = { lights, ... },
  ...
} /true/ {
  [ Event::LightStateChanged(l) for l in keys(lights) ]
}"#;
    let result = codegen_and_pretty(src);
    insta::assert_snapshot!(result);
}

// =============================================================================
// Function calls and enum variants
// =============================================================================

#[test]
fn test_codegen_function_call() {
    let result = codegen_and_pretty("observer {} /true/ { clamp(100, 0, 255); [] }");
    insta::assert_snapshot!(result);
}

// =============================================================================
// Struct literals with spread
// =============================================================================

#[test]
fn test_codegen_struct_with_spread() {
    let src = r#"mutator {
  event,
  ...
} /true/ {
  let brightness = 100;
  Event {
    inherit brightness;
    ...event
  }
}"#;
    let result = codegen_and_pretty(src);
    insta::assert_snapshot!(result);
}

// =============================================================================
// Mutator filter exit path
// =============================================================================

#[test]
fn test_codegen_mutator_filter_exit() {
    let src = r#"mutator {
  event,
  ...
} /true/ {
  event
}"#;
    let result = codegen_and_pretty(src);
    insta::assert_snapshot!(result);
}

// =============================================================================
// Constant and symbol dedup
// =============================================================================

#[test]
fn test_codegen_constant_dedup() {
    // Same constant `42` used twice should get same ConstIdx
    let result = codegen_and_pretty("observer {} /true/ { let a = 42; let b = 42; [] }");
    insta::assert_snapshot!(result);
}

#[test]
fn test_codegen_symbol_dedup() {
    // Same function name used twice should get same SymIdx
    let result =
        codegen_and_pretty("observer {} /true/ { clamp(1, 0, 10); clamp(2, 0, 10); [] }");
    insta::assert_snapshot!(result);
}

// =============================================================================
// No filter
// =============================================================================

#[test]
fn test_codegen_no_filter() {
    let result = codegen_and_pretty("observer {} { [] }");
    insta::assert_snapshot!(result);
}

// =============================================================================
// List literal
// =============================================================================

#[test]
fn test_codegen_list_literal() {
    let result = codegen_and_pretty("observer {} /true/ { [1, 2, 3]; [] }");
    insta::assert_snapshot!(result);
}
