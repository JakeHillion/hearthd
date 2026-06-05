use crate::automations::repr::pretty_print::PrettyPrint;

/// Lower a program all the way through to LIR and pretty-print it.
fn lower_and_pretty(input: &str) -> String {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    let result = crate::automations::check_program(&lowered);
    let hir = crate::automations::lower_program(&result);
    let lir = crate::automations::lower_lir::lower_lir_program(&hir);
    lir.to_pretty_string()
}

// =============================================================================
// Literal / simple body
// =============================================================================

#[test]
fn test_lower_lir_empty_list_observer() {
    let result = lower_and_pretty("observer {} /true/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 1
      L0:
        r0 = empty_list
        return r0
    ");
}

#[test]
fn test_lower_lir_let_binding() {
    let result = lower_and_pretty("observer {} /true/ { let x = 42; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 2
      L0:
        r0 = const_int 42
        r1 = empty_list
        return r1
    ");
}

// =============================================================================
// Control flow: if/else
// =============================================================================

#[test]
fn test_lower_lir_if_else() {
    let result = lower_and_pretty("observer {} /true/ { if true { [] } else { [] } }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 4
      L0:
        r1 = const_bool true
        jump_if r1 -> L1, L2
      L1:
        r2 = empty_list
        r0 = copy r2
        jump L3
      L2:
        r3 = empty_list
        r0 = copy r3
        jump L3
      L3:
        return r0
    ");
}

// =============================================================================
// List comprehension (desugars into for + push)
// =============================================================================

#[test]
fn test_lower_lir_list_comprehension() {
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
      L0:
        r1 = field r0.nodes
        r2 = const_bool true
        return r2
      body:
        regs: 8
        params:
          r0: state [State]
      L0:
        r1 = field r0.nodes
        r2 = empty_list
        r3 = call keys(r1)
        r4 = iter_init r3
        jump L1
      L1:
        iter_next r4 -> r5, L2, L3
      L2:
        r6 = variant Event::OnOffChanged(r5)
        list_push r2, r6
        jump L1
      L3:
        return r2
    ");
}

// =============================================================================
// sleep_unique: exercises Call + Await pairing
// =============================================================================

#[test]
fn test_lower_lir_sleep_unique() {
    let src = r#"observer {} /true/ {
  if await sleep_unique(5min) { [] } else { [] }
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 6
      L0:
        r1 = const_unit 5min
        r2 = call sleep_unique(r1)
        r3 = await r2
        jump_if r3 -> L1, L2
      L1:
        r4 = empty_list
        r0 = copy r4
        jump L3
      L2:
        r5 = empty_list
        r0 = copy r5
        jump L3
      L3:
        return r0
    ");
}
