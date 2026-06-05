use crate::automations::repr::pretty_print::PrettyPrint;

/// Lower a program and pretty-print the HIR. Tolerates type errors since
/// we're testing lowering, not the type checker.
fn lower_and_pretty(input: &str) -> String {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    let result = crate::automations::check_program(&lowered);
    let hir = crate::automations::lower_program(&result);
    hir.to_pretty_string()
}

// =============================================================================
// Simple expressions
// =============================================================================

#[test]
fn test_lower_empty_list_observer() {
    let result = lower_and_pretty("observer {} /true/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = empty_list [[<error>]]
          return %0
    ");
}

#[test]
fn test_lower_let_binding() {
    let result = lower_and_pretty("observer {} /true/ { let x = 42; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 42 [Int]
          %1 = empty_list [[<error>]]
          return %1
    ");
}

#[test]
fn test_lower_binary_arithmetic() {
    let result = lower_and_pretty("observer {} /true/ { 1 + 2 * 3; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 1 [Int]
          %1 = const_int 2 [Int]
          %2 = const_int 3 [Int]
          %3 = mul %1, %2 [Int]
          %4 = add %0, %3 [Int]
          %5 = empty_list [[<error>]]
          return %5
    ");
}

// =============================================================================
// Control flow
// =============================================================================

#[test]
fn test_lower_if_else() {
    let result = lower_and_pretty("observer {} /true/ { if true { [] } else { [] } }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %1 = const_bool true [Bool]
          branch %1 -> bb1, bb2
        bb1:
          %2 = empty_list [[<error>]]
          %0 = copy %2 [[<error>]]
          jump -> bb3
        bb2:
          %3 = empty_list [[<error>]]
          %0 = copy %3 [[<error>]]
          jump -> bb3
        bb3:
          return %0
    ");
}

#[test]
fn test_lower_if_no_else() {
    let result = lower_and_pretty("observer {} /true/ { if true { 42 }; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %1 = const_bool true [Bool]
          branch %1 -> bb1, bb2
        bb1:
          %2 = const_int 42 [Int]
          %0 = copy %2 [()]
          jump -> bb3
        bb2:
          %0 = unit [()]
          jump -> bb3
        bb3:
          %3 = empty_list [[<error>]]
          return %3
    ");
}

#[test]
fn test_lower_short_circuit_and() {
    let result = lower_and_pretty("observer {} /true && false/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %1 = const_bool true [Bool]
          branch %1 -> bb1, bb2
        bb1:
          %2 = const_bool false [Bool]
          %0 = copy %2 [Bool]
          jump -> bb3
        bb2:
          %0 = const_bool false [Bool]
          jump -> bb3
        bb3:
          return %0
      body:
        bb0:
          %0 = empty_list [[<error>]]
          return %0
    ");
}

#[test]
fn test_lower_short_circuit_or() {
    let result = lower_and_pretty("observer {} /true || false/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %1 = const_bool true [Bool]
          branch %1 -> bb1, bb2
        bb1:
          %0 = const_bool true [Bool]
          jump -> bb3
        bb2:
          %2 = const_bool false [Bool]
          %0 = copy %2 [Bool]
          jump -> bb3
        bb3:
          return %0
      body:
        bb0:
          %0 = empty_list [[<error>]]
          return %0
    ");
}

// =============================================================================
// Loops and comprehensions
// =============================================================================

#[test]
fn test_lower_list_comprehension() {
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
        Params:
          %0: state [State]
        bb0:
          %1 = field %0.nodes [Map<Int, Node>]
          %2 = const_bool true [Bool]
          return %2
      body:
        Params:
          %0: state [State]
        bb0:
          %1 = field %0.nodes [Map<Int, Node>]
          %2 = empty_list [[<error>]]
          %3 = call keys(%1) [[Int]]
          %4 = iter_init %3 [[Int]]
          jump -> bb1
        bb1:
          iter_next %4 -> %5, bb2, bb3
        bb2:
          %6 = variant Event::OnOffChanged(%5) [Event]
          %7 = list_push %2, %6 [()]
          jump -> bb1
        bb3:
          return %2
    ");
}

// =============================================================================
// Function calls
// =============================================================================

#[test]
fn test_lower_builtin_clamp() {
    let result = lower_and_pretty("observer {} /true/ { clamp(100, 0, 255); [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 100 [Int]
          %1 = const_int 0 [Int]
          %2 = const_int 255 [Int]
          %3 = call clamp(%0, %1, %2) [Int]
          %4 = empty_list [[<error>]]
          return %4
    ");
}

// =============================================================================
// Struct literals
// =============================================================================

#[test]
fn test_lower_struct_inherit_spread() {
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
        Params:
          %0: event [Event]
        bb0:
          %1 = const_bool true [Bool]
          return %1
      body:
        Params:
          %0: event [Event]
        bb0:
          %1 = const_int 100 [Int]
          %2 = struct Event { brightness: %1, ...%0 } [Event]
          return %2
    ");
}

// =============================================================================
// Remaining binary operators
// =============================================================================

#[test]
fn test_lower_subtraction() {
    let result = lower_and_pretty("observer {} /true/ { 10 - 3; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 10 [Int]
          %1 = const_int 3 [Int]
          %2 = sub %0, %1 [Int]
          %3 = empty_list [[<error>]]
          return %3
    ");
}

#[test]
fn test_lower_division_and_modulo() {
    let result = lower_and_pretty("observer {} /true/ { 100 / 4 + 17 % 5; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 100 [Int]
          %1 = const_int 4 [Int]
          %2 = div %0, %1 [Int]
          %3 = const_int 17 [Int]
          %4 = const_int 5 [Int]
          %5 = mod %3, %4 [Int]
          %6 = add %2, %5 [Int]
          %7 = empty_list [[<error>]]
          return %7
    ");
}

#[test]
fn test_lower_comparison_operators() {
    let result =
        lower_and_pretty("observer {} /true/ { 1 < 2; 3 <= 3; 5 > 4; 6 >= 6; 1 != 2; 1 == 1; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 1 [Int]
          %1 = const_int 2 [Int]
          %2 = lt %0, %1 [Bool]
          %3 = const_int 3 [Int]
          %4 = const_int 3 [Int]
          %5 = le %3, %4 [Bool]
          %6 = const_int 5 [Int]
          %7 = const_int 4 [Int]
          %8 = gt %6, %7 [Bool]
          %9 = const_int 6 [Int]
          %10 = const_int 6 [Int]
          %11 = ge %9, %10 [Bool]
          %12 = const_int 1 [Int]
          %13 = const_int 2 [Int]
          %14 = ne %12, %13 [Bool]
          %15 = const_int 1 [Int]
          %16 = const_int 1 [Int]
          %17 = eq %15, %16 [Bool]
          %18 = empty_list [[<error>]]
          return %18
    ");
}

// =============================================================================
// Unary operators
// =============================================================================

#[test]
fn test_lower_negation() {
    let result = lower_and_pretty("observer {} /true/ { let x = 10; -x; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 10 [Int]
          %1 = neg %0 [Int]
          %2 = empty_list [[<error>]]
          return %2
    ");
}

#[test]
fn test_lower_not() {
    let result = lower_and_pretty("observer {} /true/ { !true; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_bool true [Bool]
          %1 = not %0 [Bool]
          %2 = empty_list [[<error>]]
          return %2
    ");
}

// =============================================================================
// Literals
// =============================================================================

#[test]
fn test_lower_string_literal() {
    let result = lower_and_pretty(r#"observer {} /true/ { "hello"; [] }"#);
    insta::assert_snapshot!(result, @r#"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_string "hello" [String]
          %1 = empty_list [[<error>]]
          return %1
    "#);
}

#[test]
fn test_lower_float_literal() {
    let result = lower_and_pretty("observer {} /true/ { 1.5 + 2.5; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_float 1.5 [Float]
          %1 = const_float 2.5 [Float]
          %2 = add %0, %1 [Float]
          %3 = empty_list [[<error>]]
          return %3
    ");
}

#[test]
fn test_lower_unit_literals() {
    let result = lower_and_pretty("observer {} /true/ { 5s; 30min; 25c; 90deg; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_unit 5s [Duration]
          %1 = const_unit 30min [Duration]
          %2 = const_unit 25c [Temperature]
          %3 = const_unit 90deg [Angle]
          %4 = empty_list [[<error>]]
          return %4
    ");
}

// =============================================================================
// Field access
// =============================================================================

#[test]
fn test_lower_field_access() {
    let src = r#"observer {
  state = { nodes, ... },
  ...
} /true/ {
  nodes;
  []
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        Params:
          %0: state [State]
        bb0:
          %1 = field %0.nodes [Map<Int, Node>]
          %2 = const_bool true [Bool]
          return %2
      body:
        Params:
          %0: state [State]
        bb0:
          %1 = field %0.nodes [Map<Int, Node>]
          %2 = empty_list [[<error>]]
          return %2
    ");
}

// =============================================================================
// Non-empty list literal
// =============================================================================

#[test]
fn test_lower_list_literal() {
    let result = lower_and_pretty("observer {} /true/ { [1, 2, 3]; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 1 [Int]
          %1 = const_int 2 [Int]
          %2 = const_int 3 [Int]
          %3 = list [%0, %1, %2] [[Int]]
          %4 = empty_list [[<error>]]
          return %4
    ");
}

// =============================================================================
// Variable reference
// =============================================================================

#[test]
fn test_lower_variable_reference() {
    let result = lower_and_pretty("observer {} /true/ { let x = 42; let y = x; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 42 [Int]
          %1 = empty_list [[<error>]]
          return %1
    ");
}

// =============================================================================
// Nested if/else
// =============================================================================

#[test]
fn test_lower_nested_if() {
    let result = lower_and_pretty(
        "observer {} /true/ { if true { if false { 1 } else { 2 } } else { 3 }; [] }",
    );
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %1 = const_bool true [Bool]
          branch %1 -> bb1, bb2
        bb1:
          %3 = const_bool false [Bool]
          branch %3 -> bb4, bb5
        bb2:
          %6 = const_int 3 [Int]
          %0 = copy %6 [Int]
          jump -> bb3
        bb3:
          %7 = empty_list [[<error>]]
          return %7
        bb4:
          %4 = const_int 1 [Int]
          %2 = copy %4 [Int]
          jump -> bb6
        bb5:
          %5 = const_int 2 [Int]
          %2 = copy %5 [Int]
          jump -> bb6
        bb6:
          %0 = copy %2 [Int]
          jump -> bb3
    ");
}

// =============================================================================
// Mutator filter (exit returns event)
// =============================================================================

#[test]
fn test_lower_mutator_filter_exit() {
    let src = r#"mutator {
  event,
  ...
} /true/ {
  event
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: mutator
      filter:
        Params:
          %0: event [Event]
        bb0:
          %1 = const_bool true [Bool]
          return %1
      body:
        Params:
          %0: event [Event]
        bb0:
          return %0
    ");
}

// =============================================================================
// Nested short-circuit (a && b || c)
// =============================================================================

#[test]
fn test_lower_nested_short_circuit() {
    let result = lower_and_pretty("observer {} /true && false || true/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %2 = const_bool true [Bool]
          branch %2 -> bb1, bb2
        bb1:
          %3 = const_bool false [Bool]
          %1 = copy %3 [Bool]
          jump -> bb3
        bb2:
          %1 = const_bool false [Bool]
          jump -> bb3
        bb3:
          branch %1 -> bb4, bb5
        bb4:
          %0 = const_bool true [Bool]
          jump -> bb6
        bb5:
          %4 = const_bool true [Bool]
          %0 = copy %4 [Bool]
          jump -> bb6
        bb6:
          return %0
      body:
        bb0:
          %0 = empty_list [[<error>]]
          return %0
    ");
}

// =============================================================================
// Early return
// =============================================================================

#[test]
fn test_lower_early_return() {
    let result = lower_and_pretty("observer {} /true/ { return []; 42; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = empty_list [[<error>]]
          return %0
        bb1:
          return %0
    ");
}

// =============================================================================
// Multiple statements
// =============================================================================

#[test]
fn test_lower_multiple_lets_and_arithmetic() {
    let result = lower_and_pretty("observer {} /true/ { let a = 1; let b = 2; let c = a + b; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        bb0:
          %0 = const_bool true [Bool]
          return %0
      body:
        bb0:
          %0 = const_int 1 [Int]
          %1 = const_int 2 [Int]
          %2 = add %0, %1 [Int]
          %3 = empty_list [[<error>]]
          return %3
    ");
}

// =============================================================================
// List comprehension with filter
// =============================================================================

#[test]
fn test_lower_list_comprehension_with_filter() {
    let src = r#"observer {
  state = { nodes, ... },
  ...
} /true/ {
  [ Event::OnOffChanged(l) for l in keys(nodes) if true ]
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        Params:
          %0: state [State]
        bb0:
          %1 = field %0.nodes [Map<Int, Node>]
          %2 = const_bool true [Bool]
          return %2
      body:
        Params:
          %0: state [State]
        bb0:
          %1 = field %0.nodes [Map<Int, Node>]
          %2 = empty_list [[<error>]]
          %3 = call keys(%1) [[Int]]
          %4 = iter_init %3 [[Int]]
          jump -> bb1
        bb1:
          iter_next %4 -> %5, bb2, bb3
        bb2:
          %7 = const_bool true [Bool]
          branch %7 -> bb4, bb5
        bb3:
          return %2
        bb4:
          %8 = variant Event::OnOffChanged(%5) [Event]
          %9 = list_push %2, %8 [()]
          %10 = unit [()]
          %6 = copy %10 [()]
          jump -> bb6
        bb5:
          %6 = unit [()]
          jump -> bb6
        bb6:
          jump -> bb1
    ");
}

// =============================================================================
// Nested pattern destructuring
// =============================================================================

#[test]
fn test_lower_nested_pattern() {
    let src = r#"observer {
  event,
  state = {
    nodes,
    by_entity_id,
    ...
  },
  ...
} /true/ {
  nodes;
  by_entity_id;
  []
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        Params:
          %0: event [Event]
          %1: state [State]
        bb0:
          %2 = field %1.nodes [Map<Int, Node>]
          %3 = field %1.by_entity_id [Map<String, Int>]
          %4 = const_bool true [Bool]
          return %4
      body:
        Params:
          %0: event [Event]
          %1: state [State]
        bb0:
          %2 = field %1.nodes [Map<Int, Node>]
          %3 = field %1.by_entity_id [Map<String, Int>]
          %4 = empty_list [[<error>]]
          return %4
    ");
}

// =============================================================================
// Integration: design doc examples
// =============================================================================

#[test]
fn test_lower_lights_off_observer() {
    let src = r#"observer {
  event,
  state = {
    nodes,
    ...
  },
  ...
} /true/ {
  [ Event::OnOffChanged(l) for l in keys(nodes) ]
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        Params:
          %0: event [Event]
          %1: state [State]
        bb0:
          %2 = field %1.nodes [Map<Int, Node>]
          %3 = const_bool true [Bool]
          return %3
      body:
        Params:
          %0: event [Event]
          %1: state [State]
        bb0:
          %2 = field %1.nodes [Map<Int, Node>]
          %3 = empty_list [[<error>]]
          %4 = call keys(%2) [[Int]]
          %5 = iter_init %4 [[Int]]
          jump -> bb1
        bb1:
          iter_next %5 -> %6, bb2, bb3
        bb2:
          %7 = variant Event::OnOffChanged(%6) [Event]
          %8 = list_push %3, %7 [()]
          jump -> bb1
        bb3:
          return %3
    ");
}

#[test]
fn test_lower_mutator_with_computation() {
    let src = r#"mutator {
  event,
  ...
} /true/ {
  let brightness = clamp(100 * 2, 0, 255);
  Event {
    inherit brightness;
    ...event
  }
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: mutator
      filter:
        Params:
          %0: event [Event]
        bb0:
          %1 = const_bool true [Bool]
          return %1
      body:
        Params:
          %0: event [Event]
        bb0:
          %1 = const_int 100 [Int]
          %2 = const_int 2 [Int]
          %3 = mul %1, %2 [Int]
          %4 = const_int 0 [Int]
          %5 = const_int 255 [Int]
          %6 = call clamp(%3, %4, %5) [Int]
          %7 = struct Event { brightness: %6, ...%0 } [Event]
          return %7
    ");
}

#[test]
fn test_lower_no_filter() {
    let result = lower_and_pretty("observer {} { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      body:
        bb0:
          %0 = empty_list [[<error>]]
          return %0
    ");
}

#[test]
fn test_lower_observer_if_else_with_events() {
    let src = r#"observer {
  event,
  state = { nodes, ... },
  ...
} /true/ {
  if true {
    [ Event::OnOffChanged(l) for l in keys(nodes) ]
  } else {
    []
  }
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      filter:
        Params:
          %0: event [Event]
          %1: state [State]
        bb0:
          %2 = field %1.nodes [Map<Int, Node>]
          %3 = const_bool true [Bool]
          return %3
      body:
        Params:
          %0: event [Event]
          %1: state [State]
        bb0:
          %2 = field %1.nodes [Map<Int, Node>]
          %4 = const_bool true [Bool]
          branch %4 -> bb1, bb2
        bb1:
          %5 = empty_list [[<error>]]
          %6 = call keys(%2) [[Int]]
          %7 = iter_init %6 [[Int]]
          jump -> bb4
        bb2:
          %11 = empty_list [[<error>]]
          %3 = copy %11 [[Event]]
          jump -> bb3
        bb3:
          return %3
        bb4:
          iter_next %7 -> %8, bb5, bb6
        bb5:
          %9 = variant Event::OnOffChanged(%8) [Event]
          %10 = list_push %5, %9 [()]
          jump -> bb4
        bb6:
          %3 = copy %5 [[Event]]
          jump -> bb3
    ");
}
