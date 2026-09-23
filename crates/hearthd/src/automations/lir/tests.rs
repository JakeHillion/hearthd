use crate::automations::pretty_print::PrettyPrint;

/// Lower a program all the way through to LIR and pretty-print it.
fn lower_and_pretty(input: &str) -> String {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    let result = crate::automations::check_program(&lowered);
    let hir = crate::automations::lower_program(&result);
    let lir = crate::automations::lir::lower_lir_program(&hir);
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

// =============================================================================
// Literals
// =============================================================================

#[test]
fn test_lower_lir_binary_arithmetic() {
    let result = lower_and_pretty("observer {} /true/ { 1 + 2 * 3; [] }");
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
        r0 = const_int 1
        r1 = const_int 2
        r2 = const_int 3
        r3 = mul_int r1, r2
        r4 = add_int r0, r3
        r5 = empty_list
        return r5
    ");
}

#[test]
fn test_lower_lir_float_literal() {
    let result = lower_and_pretty("observer {} /true/ { 1.5 + 2.5; [] }");
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
        r0 = const_float 1.5
        r1 = const_float 2.5
        r2 = add_float r0, r1
        r3 = empty_list
        return r3
    ");
}

#[test]
fn test_lower_lir_string_literal() {
    let result = lower_and_pretty(r#"observer {} /true/ { "hello"; [] }"#);
    insta::assert_snapshot!(result, @r#"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 2
      L0:
        r0 = const_string "hello"
        r1 = empty_list
        return r1
    "#);
}

#[test]
fn test_lower_lir_list_literal() {
    let result = lower_and_pretty("observer {} /true/ { [1, 2, 3]; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 5
      L0:
        r0 = const_int 1
        r1 = const_int 2
        r2 = const_int 3
        r3 = list [r0, r1, r2]
        r4 = empty_list
        return r4
    ");
}

// =============================================================================
// Unary operators
// =============================================================================

#[test]
fn test_lower_lir_negation() {
    let result = lower_and_pretty("observer {} /true/ { let x = 10; -x; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 3
      L0:
        r0 = const_int 10
        r1 = neg r0
        r2 = empty_list
        return r2
    ");
}

#[test]
fn test_lower_lir_not() {
    let result = lower_and_pretty("observer {} /true/ { !true; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 3
      L0:
        r0 = const_bool true
        r1 = not r0
        r2 = empty_list
        return r2
    ");
}

#[test]
fn test_lower_lir_deref() {
    let result = lower_and_pretty("observer {} /true/ { let x = 10; *x; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 3
      L0:
        r0 = const_int 10
        r1 = deref r0
        r2 = empty_list
        return r2
    ");
}

// =============================================================================
// Field access
// =============================================================================

#[test]
fn test_lower_lir_optional_field() {
    let src = r#"observer {
  event,
  ...
} /true/ {
  event?.location;
  []
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 2
        params:
          r0: event [Event]
      L0:
        r1 = const_bool true
        return r1
      body:
        regs: 3
        params:
          r0: event [Event]
      L0:
        r1 = optional_field r0?.location
        r2 = empty_list
        return r2
    ");
}

// =============================================================================
// if without else: the merge register is filled with Unit
// =============================================================================

#[test]
fn test_lower_lir_if_no_else() {
    let result = lower_and_pretty("observer {} /true/ { if true { 42 }; [] }");
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
        r2 = const_int 42
        r0 = copy r2
        jump L3
      L2:
        r0 = unit
        jump L3
      L3:
        r3 = empty_list
        return r3
    ");
}

// =============================================================================
// Struct construction: mutator kind, inherit (Set) and spread fields
// =============================================================================

#[test]
fn test_lower_lir_struct_inherit_spread() {
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
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: mutator
      filter:
        regs: 2
        params:
          r0: event [Event]
      L0:
        r1 = const_bool true
        return r1
      body:
        regs: 3
        params:
          r0: event [Event]
      L0:
        r1 = const_int 100
        r2 = struct Event { brightness: r1, ...r0 }
        return r2
    ");
}

// =============================================================================
// Program shapes: absent filter, and templates
// =============================================================================

#[test]
fn test_lower_lir_no_filter() {
    let result = lower_and_pretty("observer {} { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      body:
        regs: 1
      L0:
        r0 = empty_list
        return r0
    ");
}

#[test]
fn test_lower_lir_template() {
    let src = r#"{ room: String }: [
        observer { state = { nodes, ... }, ... } /true/ { [] },
        mutator { event, ... } /true/ { event }
    ]"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Template:
      Params:
        Param: room
          Type::Named: String
      Automations:
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
            regs: 3
            params:
              r0: state [State]
          L0:
            r1 = field r0.nodes
            r2 = empty_list
            return r2
        Automation: mutator
          filter:
            regs: 2
            params:
              r0: event [Event]
          L0:
            r1 = const_bool true
            return r1
          body:
            regs: 1
            params:
              r0: event [Event]
          L0:
            return r0
    ");
}

// =============================================================================
// Numeric promotion
// =============================================================================

/// A `Float` on either side contaminates, so the `Int` operand is widened
/// into a scratch register before a float opcode consumes it. The register
/// count grows past the highest `Tmp` to hold it.
#[test]
fn test_lower_lir_mixed_arithmetic_int_lhs() {
    let result = lower_and_pretty("observer {} /true/ { 1 + 2.5; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 5
      L0:
        r0 = const_int 1
        r1 = const_float 2.5
        r4 = to_float r0
        r2 = add_float r4, r1
        r3 = empty_list
        return r3
    ");
}

#[test]
fn test_lower_lir_mixed_arithmetic_int_rhs() {
    let result = lower_and_pretty("observer {} /true/ { 2.5 + 1; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
      L0:
        r0 = const_bool true
        return r0
      body:
        regs: 5
      L0:
        r0 = const_float 2.5
        r1 = const_int 1
        r4 = to_float r1
        r2 = add_float r0, r4
        r3 = empty_list
        return r3
    ");
}

/// A comparison's operands are promoted the same way, though its result
/// type says nothing about them.
#[test]
fn test_lower_lir_mixed_comparison() {
    let result = lower_and_pretty("observer {} /1 < 2.5/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 4
      L0:
        r0 = const_int 1
        r1 = const_float 2.5
        r3 = to_float r0
        r2 = lt_float r3, r1
        return r2
      body:
        regs: 1
      L0:
        r0 = empty_list
        return r0
    ");
}

/// Equality stays polymorphic, so neither side is promoted and no opcode
/// commits to a type.
#[test]
fn test_lower_lir_mixed_equality() {
    let result = lower_and_pretty("observer {} /1 == 2.5/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 3
      L0:
        r0 = const_int 1
        r1 = const_float 2.5
        r2 = eq r0, r1
        return r2
      body:
        regs: 1
      L0:
        r0 = empty_list
        return r0
    ");
}
