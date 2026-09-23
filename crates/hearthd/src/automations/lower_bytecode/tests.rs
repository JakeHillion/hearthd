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
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          load_const_bool    r0, true
          return             r0
      body:
        regs: 1
        code:
          empty_list         r0
          return             r0
    ");
}

#[test]
fn test_lower_bytecode_let_binding() {
    let result = lower_and_pretty("observer {} /true/ { let x = 42; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          load_const_bool    r0, true
          return             r0
      body:
        regs: 2
        consts:
          #0 = int 42
        code:
          load_const_int     r0, #0 (int 42)
          empty_list         r1
          return             r1
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 4
        code:
          load_const_bool    r1, true
          jump_if            r1, l0, l1
        l0:
          empty_list         r2
          copy               r0, r2
          jump               l2
        l1:
          empty_list         r3
          copy               r0, r3
          jump               l2
        l2:
          return             r0
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
          field              r1, r0, #0 (nodes)
          load_const_bool    r2, true
          return             r2
      body:
        regs: 8
        params:
          r0: state [State]
        consts:
          #0 = ident nodes
          #1 = ident Event
          #2 = ident OnOffChanged
        code:
          field              r1, r0, #0 (nodes)
          empty_list         r2
          call               r3, keys, [r1]
          iter_init          r4, r3
          jump               l0
        l0:
          iter_next          r4, r5, l1, l2
        l1:
          variant            r6, #1 (Event), #2 (OnOffChanged), [r5]
          list_push          r2, r6
          jump               l0
        l2:
          return             r2
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 6
        consts:
          #0 = unit 5min
        code:
          load_const_unit    r1, #0 (5min)
          call               r2, sleep_unique, [r1]
          await              r3, r2
          jump_if            r3, l0, l1
        l0:
          empty_list         r4
          copy               r0, r4
          jump               l2
        l1:
          empty_list         r5
          copy               r0, r5
          jump               l2
        l2:
          return             r0
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 6
        consts:
          #0 = int 1
          #1 = int 2
          #2 = int 3
        code:
          load_const_int     r0, #0 (int 1)
          load_const_int     r1, #1 (int 2)
          load_const_int     r2, #2 (int 3)
          mul_int            r3, r1, r2
          add_int            r4, r0, r3
          empty_list         r5
          return             r5
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 4
        consts:
          #0 = float 1.5
          #1 = float 2.5
        code:
          load_const_float   r0, #0 (float 1.5)
          load_const_float   r1, #1 (float 2.5)
          add_float          r2, r0, r1
          empty_list         r3
          return             r3
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 2
        consts:
          #0 = string "hello"
        code:
          load_const_string  r0, #0 ("hello")
          empty_list         r1
          return             r1
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 5
        consts:
          #0 = int 1
          #1 = int 2
          #2 = int 3
        code:
          load_const_int     r0, #0 (int 1)
          load_const_int     r1, #1 (int 2)
          load_const_int     r2, #2 (int 3)
          list               r3, [r0, r1, r2]
          empty_list         r4
          return             r4
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 3
        consts:
          #0 = int 10
        code:
          load_const_int     r0, #0 (int 10)
          neg                r1, r0
          empty_list         r2
          return             r2
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 3
        code:
          load_const_bool    r0, true
          not                r1, r0
          empty_list         r2
          return             r2
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 3
        consts:
          #0 = int 10
        code:
          load_const_int     r0, #0 (int 10)
          deref              r1, r0
          empty_list         r2
          return             r2
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
          load_const_bool    r1, true
          return             r1
      body:
        regs: 3
        params:
          r0: event [Event]
        consts:
          #0 = ident location
        code:
          optional_field     r1, r0, #0 (location)
          empty_list         r2
          return             r2
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 4
        consts:
          #0 = int 42
        code:
          load_const_bool    r1, true
          jump_if            r1, l0, l1
        l0:
          load_const_int     r2, #0 (int 42)
          copy               r0, r2
          jump               l2
        l1:
          unit               r0
          jump               l2
        l2:
          empty_list         r3
          return             r3
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
          load_const_bool    r1, true
          return             r1
      body:
        regs: 3
        params:
          r0: event [Event]
        consts:
          #0 = int 100
          #1 = ident Event
          #2 = ident brightness
        code:
          load_const_int     r1, #0 (int 100)
          struct             r2, #1 (Event), { brightness: r1, ...r0 }
          return             r2
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 4
        consts:
          #0 = int 1
        code:
          load_const_int     r0, #0 (int 1)
          load_const_int     r1, #0 (int 1)
          add_int            r2, r0, r1
          empty_list         r3
          return             r3
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
          load_const_bool    r0, true
          return             r0
      body:
        regs: 9
        consts:
          #0 = int 1
          #1 = int 0
          #2 = int 9
          #3 = int 2
        code:
          load_const_int     r0, #0 (int 1)
          load_const_int     r1, #1 (int 0)
          load_const_int     r2, #2 (int 9)
          call               r3, clamp, [r0, r1, r2]
          load_const_int     r4, #3 (int 2)
          load_const_int     r5, #1 (int 0)
          load_const_int     r6, #2 (int 9)
          call               r7, clamp, [r4, r5, r6]
          empty_list         r8
          return             r8
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
          empty_list         r0
          return             r0
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
              field              r1, r0, #0 (nodes)
              load_const_bool    r2, true
              return             r2
          body:
            regs: 3
            params:
              r0: state [State]
            consts:
              #0 = ident nodes
            code:
              field              r1, r0, #0 (nodes)
              empty_list         r2
              return             r2
        Automation: mutator
          filter:
            regs: 2
            params:
              r0: event [Event]
            code:
              load_const_bool    r1, true
              return             r1
          body:
            regs: 1
            params:
              r0: event [Event]
            code:
              return             r0
    ");
}

#[test]
fn test_lower_bytecode_mixed_arithmetic() {
    let result = lower_and_pretty("observer {} /true/ { 1 + 2.5; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        regs: 1
        code:
          load_const_bool    r0, true
          return             r0
      body:
        regs: 5
        consts:
          #0 = int 1
          #1 = float 2.5
        code:
          load_const_int     r0, #0 (int 1)
          load_const_float   r1, #1 (float 2.5)
          to_float           r4, r0
          add_float          r2, r4, r1
          empty_list         r3
          return             r3
    ");
}
