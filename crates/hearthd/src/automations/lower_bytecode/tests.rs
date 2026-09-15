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
          #1 = ident Event
          #2 = ident OnOffChanged
        code:
          0000: field              r1, r0, #0 (nodes)
          0013: empty_list         r2
          0018: call               r3, keys, [r1]
          0032: iter_init          r4, r3
          0041: jump               0046
          0046: iter_next          r4, r5, 0063, 0098
          0063: variant            r6, #1 (Event), #2 (OnOffChanged), [r5]
          0084: list_push          r2, r6
          0093: jump               0046
          0098: return             r2
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
        code:
          0000: load_const_unit    r1, #0 (5min)
          0009: call               r2, sleep_unique, [r1]
          0023: await              r3, r2
          0032: jump_if            r3, 0045, 0064
          0045: empty_list         r4
          0050: copy               r0, r4
          0059: jump               0083
          0064: empty_list         r5
          0069: copy               r0, r5
          0078: jump               0083
          0083: return             r0
    ");
}

// =============================================================================
// Literals
// =============================================================================

#[test]
fn test_lower_bytecode_binary_arithmetic() {
    let result = lower_and_pretty("observer {} /true/ { 1 + 2 * 3; [] }");
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
          #0 = int 1
          #1 = int 2
          #2 = int 3
        code:
          0000: load_const_int     r0, #0 (int 1)
          0009: load_const_int     r1, #1 (int 2)
          0018: load_const_int     r2, #2 (int 3)
          0027: binop              r3, mul_int, r1, r2
          0041: binop              r4, add_int, r0, r3
          0055: empty_list         r5
          0060: return             r5
    ");
}

#[test]
fn test_lower_bytecode_float_literal() {
    let result = lower_and_pretty("observer {} /true/ { 1.5 + 2.5; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 4
        consts:
          #0 = float 1.5
          #1 = float 2.5
        code:
          0000: load_const_float   r0, #0 (float 1.5)
          0009: load_const_float   r1, #1 (float 2.5)
          0018: binop              r2, add_float, r0, r1
          0032: empty_list         r3
          0037: return             r3
    ");
}

/// A mixed `Int`/`Float` addition monomorphises to `add_float`, promoting
/// the `Int` operand through `to_float` onto a fresh register past the HIR
/// temps.
#[test]
fn test_lower_bytecode_mixed_int_float() {
    let result = lower_and_pretty("observer {} /true/ { 1 + 2.5; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 5
        consts:
          #0 = int 1
          #1 = float 2.5
        code:
          0000: load_const_int     r0, #0 (int 1)
          0009: load_const_float   r1, #1 (float 2.5)
          0018: to_float           r4, r0
          0027: binop              r2, add_float, r4, r1
          0041: empty_list         r3
          0046: return             r3
    ");
}

#[test]
fn test_lower_bytecode_string_literal() {
    let result = lower_and_pretty(r#"observer {} /true/ { "hello"; [] }"#);
    insta::assert_snapshot!(result, @r#"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 2
        consts:
          #0 = string "hello"
        code:
          0000: load_const_string  r0, #0 ("hello")
          0009: empty_list         r1
          0014: return             r1
    "#);
}

#[test]
fn test_lower_bytecode_list_literal() {
    let result = lower_and_pretty("observer {} /true/ { [1, 2, 3]; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 5
        consts:
          #0 = int 1
          #1 = int 2
          #2 = int 3
        code:
          0000: load_const_int     r0, #0 (int 1)
          0009: load_const_int     r1, #1 (int 2)
          0018: load_const_int     r2, #2 (int 3)
          0027: list               r3, [r0, r1, r2]
          0048: empty_list         r4
          0053: return             r4
    ");
}

// =============================================================================
// Unary operators
// =============================================================================

#[test]
fn test_lower_bytecode_negation() {
    let result = lower_and_pretty("observer {} /true/ { let x = 10; -x; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 3
        consts:
          #0 = int 10
        code:
          0000: load_const_int     r0, #0 (int 10)
          0009: neg                r1, r0
          0018: empty_list         r2
          0023: return             r2
    ");
}

#[test]
fn test_lower_bytecode_not() {
    let result = lower_and_pretty("observer {} /true/ { !true; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 3
        code:
          0000: load_const_bool    r0, true
          0006: not                r1, r0
          0015: empty_list         r2
          0020: return             r2
    ");
}

#[test]
fn test_lower_bytecode_deref() {
    let result = lower_and_pretty("observer {} /true/ { let x = 10; *x; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 3
        consts:
          #0 = int 10
        code:
          0000: load_const_int     r0, #0 (int 10)
          0009: deref              r1, r0
          0018: empty_list         r2
          0023: return             r2
    ");
}

// =============================================================================
// Field access
// =============================================================================

#[test]
fn test_lower_bytecode_optional_field() {
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
        code:
          0000: load_const_bool    r1, true
          0006: return             r1
      body:
        regs: 3
        params:
          r0: event [Event]
        consts:
          #0 = ident location
        code:
          0000: optional_field     r1, r0, #0 (location)
          0013: empty_list         r2
          0018: return             r2
    ");
}

// =============================================================================
// if without else: the merge register is filled with Unit
// =============================================================================

#[test]
fn test_lower_bytecode_if_no_else() {
    let result = lower_and_pretty("observer {} /true/ { if true { 42 }; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 4
        consts:
          #0 = int 42
        code:
          0000: load_const_bool    r1, true
          0006: jump_if            r1, 0019, 0042
          0019: load_const_int     r2, #0 (int 42)
          0028: copy               r0, r2
          0037: jump               0052
          0042: unit               r0
          0047: jump               0052
          0052: empty_list         r3
          0057: return             r3
    ");
}

// =============================================================================
// Struct construction: per-field Set / Spread tag bytes
// =============================================================================

#[test]
fn test_lower_bytecode_struct_inherit_spread() {
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
        code:
          0000: load_const_bool    r1, true
          0006: return             r1
      body:
        regs: 3
        params:
          r0: event [Event]
        consts:
          #0 = int 100
          #1 = ident Event
          #2 = ident brightness
        code:
          0000: load_const_int     r1, #0 (int 100)
          0009: struct             r2, #1 (Event), { brightness: r1, ...r0 }
          0036: return             r2
    ");
}

// =============================================================================
// Constant pool interning: repeated values share a single slot
// =============================================================================

#[test]
fn test_lower_bytecode_interns_repeated_int() {
    let result = lower_and_pretty("observer {} /true/ { 1 + 1; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 4
        consts:
          #0 = int 1
        code:
          0000: load_const_int     r0, #0 (int 1)
          0009: load_const_int     r1, #0 (int 1)
          0018: binop              r2, add_int, r0, r1
          0032: empty_list         r3
          0037: return             r3
    ");
}

#[test]
fn test_lower_bytecode_interns_repeated_ident() {
    let result = lower_and_pretty("observer {} /true/ { clamp(1, 0, 9); clamp(2, 0, 9); [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          0000: load_const_bool    r0, true
          0006: return             r0
      body:
        regs: 9
        consts:
          #0 = int 1
          #1 = int 0
          #2 = int 9
          #3 = int 2
        code:
          0000: load_const_int     r0, #0 (int 1)
          0009: load_const_int     r1, #1 (int 0)
          0018: load_const_int     r2, #2 (int 9)
          0027: call               r3, clamp, [r0, r1, r2]
          0049: load_const_int     r4, #3 (int 2)
          0058: load_const_int     r5, #1 (int 0)
          0067: load_const_int     r6, #2 (int 9)
          0076: call               r7, clamp, [r4, r5, r6]
          0098: empty_list         r8
          0103: return             r8
    ");
}

// =============================================================================
// Program shapes: absent filter, and templates
// =============================================================================

#[test]
fn test_lower_bytecode_no_filter() {
    let result = lower_and_pretty("observer {} { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      body:
        regs: 1
        code:
          0000: empty_list         r0
          0005: return             r0
    ");
}

#[test]
fn test_lower_bytecode_template() {
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
            consts:
              #0 = ident nodes
            code:
              0000: field              r1, r0, #0 (nodes)
              0013: load_const_bool    r2, true
              0019: return             r2
          body:
            regs: 3
            params:
              r0: state [State]
            consts:
              #0 = ident nodes
            code:
              0000: field              r1, r0, #0 (nodes)
              0013: empty_list         r2
              0018: return             r2
        Automation: mutator
          filter:
            regs: 2
            params:
              r0: event [Event]
            code:
              0000: load_const_bool    r1, true
              0006: return             r1
          body:
            regs: 1
            params:
              r0: event [Event]
            code:
              0000: return             r0
    ");
}
