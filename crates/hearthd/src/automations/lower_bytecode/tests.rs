use crate::automations::repr::pretty_print::PrettyPrint;

/// Compile a program all the way to bytecode and pretty-print its
/// disassembly.
fn lower_and_pretty(input: &str) -> String {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    let result = crate::automations::check_program(&lowered);
    let hir = crate::automations::lower_program(&result);
    let lir = crate::automations::lower_lir::lower_lir_program(&hir);
    let bc = crate::automations::lower_bytecode::lower_bytecode_program(&lir);
    bc.to_pretty_string()
}

// =============================================================================
// Literal / simple body
// =============================================================================

#[test]
fn test_lower_bytecode_empty_list_observer() {
    let result = lower_and_pretty("observer {} /true/ { [] }");
    insta::assert_snapshot!(result, @r"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 1
        code:
          0000: empty_list         r0
          0005: return             r0
    ");
}

#[test]
fn test_lower_bytecode_let_binding() {
    let result = lower_and_pretty("observer {} /true/ { let x = 42; [] }");
    insta::assert_snapshot!(result, @r"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 2
        consts:
          #0 = int 42
        code:
          0000: load_const_int     r0, #0 (int 42)
          0009: empty_list         r1
          0014: return             r1
    ");
}

// =============================================================================
// Control flow: if/else (covers JumpIf and Jump backpatching)
// =============================================================================

#[test]
fn test_lower_bytecode_if_else() {
    let result = lower_and_pretty("observer {} /true/ { if true { [] } else { [] } }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 4
        code:
          0000: load_const_bool    r1, true
          0006: jump_if            r1, 0019, 0038
          0019: empty_list         r2
          0024: copy               r0, r2
          0033: jump               0057
          0038: empty_list         r3
          0043: copy               r0, r3
          0052: jump               0057
          0057: return             r0
    ");
}

// =============================================================================
// List comprehension (exercises IterInit / IterNext / ListPush / Variant)
// =============================================================================

#[test]
fn test_lower_bytecode_list_comprehension() {
    let src = r#"observer {
  state = { nodes, ... },
  ...
} /true/ {
  [ Event::OnOffChanged(l) for l in keys(nodes) ]
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 3
        params:
          r0: state [State]
        consts:
          #0 = ident nodes
        code:
          0000: field              r1, r0, #0 (nodes)
          0013: load_const_bool    r2, true
          0019: return             r2
      body:
        regs: 8
        params:
          r0: state [State]
        consts:
          #0 = ident nodes
          #1 = ident keys
          #2 = ident Event
          #3 = ident OnOffChanged
        code:
          0000: field              r1, r0, #0 (nodes)
          0013: empty_list         r2
          0018: call               r3, #1 (keys), [r1]
          0035: iter_init          r4, r3
          0044: jump               0049
          0049: iter_next          r4, r5, 0066, 0101
          0066: variant            r6, #2 (Event), #3 (OnOffChanged), [r5]
          0087: list_push          r2, r6
          0096: jump               0049
          0101: return             r2
    ");
}

// =============================================================================
// sleep_unique: exercises Call + Await pairing in bytecode form
// =============================================================================

#[test]
fn test_lower_bytecode_sleep_unique() {
    let src = r#"observer {} /true/ {
  if await sleep_unique(5min) { [] } else { [] }
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 6
        consts:
          #0 = unit 5min
          #1 = ident sleep_unique
        code:
          0000: load_const_unit    r1, #0 (5min)
          0009: call               r2, #1 (sleep_unique), [r1]
          0026: await              r3, r2
          0035: jump_if            r3, 0048, 0067
          0048: empty_list         r4
          0053: copy               r0, r4
          0062: jump               0086
          0067: empty_list         r5
          0072: copy               r0, r5
          0081: jump               0086
          0086: return             r0
    ");
}
