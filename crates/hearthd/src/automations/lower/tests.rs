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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = empty_list [[<error>]]
        return %2
      bb2:
        %1 = empty_list [[Event]]
        return %1
    ");
}

#[test]
fn test_lower_let_binding() {
    let result = lower_and_pretty("observer {} /true/ { let x = 42; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 42 [Int]
        %3 = empty_list [[<error>]]
        return %3
      bb2:
        %1 = empty_list [[Event]]
        return %1
    ");
}

#[test]
fn test_lower_binary_arithmetic() {
    let result = lower_and_pretty("observer {} /true/ { 1 + 2 * 3; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 1 [Int]
        %3 = const_int 2 [Int]
        %4 = const_int 3 [Int]
        %5 = mul %3, %4 [Int]
        %6 = add %2, %5 [Int]
        %7 = empty_list [[<error>]]
        return %7
      bb2:
        %1 = empty_list [[Event]]
        return %1
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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %3 = const_bool true [Bool]
        branch %3 -> bb3, bb4
      bb2:
        %1 = empty_list [[Event]]
        return %1
      bb3:
        %4 = empty_list [[<error>]]
        %2 = copy %4 [[<error>]]
        jump -> bb5
      bb4:
        %5 = empty_list [[<error>]]
        %2 = copy %5 [[<error>]]
        jump -> bb5
      bb5:
        return %2
    ");
}

#[test]
fn test_lower_if_no_else() {
    let result = lower_and_pretty("observer {} /true/ { if true { 42 }; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %3 = const_bool true [Bool]
        branch %3 -> bb3, bb4
      bb2:
        %1 = empty_list [[Event]]
        return %1
      bb3:
        %4 = const_int 42 [Int]
        %2 = copy %4 [()]
        jump -> bb5
      bb4:
        %2 = unit [()]
        jump -> bb5
      bb5:
        %5 = empty_list [[<error>]]
        return %5
    ");
}

#[test]
fn test_lower_short_circuit_and() {
    let result = lower_and_pretty("observer {} /true && false/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %1 = const_bool true [Bool]
        branch %1 -> bb3, bb4
      bb1:
        %4 = empty_list [[<error>]]
        return %4
      bb2:
        %3 = empty_list [[Event]]
        return %3
      bb3:
        %2 = const_bool false [Bool]
        %0 = copy %2 [Bool]
        jump -> bb5
      bb4:
        %0 = const_bool false [Bool]
        jump -> bb5
      bb5:
        branch %0 -> bb1, bb2
    ");
}

#[test]
fn test_lower_short_circuit_or() {
    let result = lower_and_pretty("observer {} /true || false/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %1 = const_bool true [Bool]
        branch %1 -> bb3, bb4
      bb1:
        %4 = empty_list [[<error>]]
        return %4
      bb2:
        %3 = empty_list [[Event]]
        return %3
      bb3:
        %0 = const_bool true [Bool]
        jump -> bb5
      bb4:
        %2 = const_bool false [Bool]
        %0 = copy %2 [Bool]
        jump -> bb5
      bb5:
        branch %0 -> bb1, bb2
    ");
}

// =============================================================================
// Loops and comprehensions
// =============================================================================

#[test]
fn test_lower_list_comprehension() {
    let src = r#"observer {
  state = { lights, ... },
  ...
} /true/ {
  [ Event::LightStateChanged(l) for l in keys(lights) ]
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      Params:
        %0: state [State]
      bb0:
        %1 = field %0.lights [Map<String, LightState>]
        %2 = const_bool true [Bool]
        branch %2 -> bb1, bb2
      bb1:
        %4 = empty_list [[<error>]]
        %5 = call keys(%1) [[String]]
        %6 = iter_init %5 [[String]]
        jump -> bb3
      bb2:
        %3 = empty_list [[Event]]
        return %3
      bb3:
        iter_next %6 -> %7, bb4, bb5
      bb4:
        %8 = variant Event::LightStateChanged(%7) [Event]
        %9 = list_push %4, %8 [()]
        jump -> bb3
      bb5:
        return %4
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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 100 [Int]
        %3 = const_int 0 [Int]
        %4 = const_int 255 [Int]
        %5 = call clamp(%2, %3, %4) [Int]
        %6 = empty_list [[<error>]]
        return %6
      bb2:
        %1 = empty_list [[Event]]
        return %1
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
      Params:
        %0: event [Event]
      bb0:
        %1 = const_bool true [Bool]
        branch %1 -> bb1, bb2
      bb1:
        %2 = const_int 100 [Int]
        %3 = struct Event { brightness: %2, ...%0 } [Event]
        return %3
      bb2:
        return %0
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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 10 [Int]
        %3 = const_int 3 [Int]
        %4 = sub %2, %3 [Int]
        %5 = empty_list [[<error>]]
        return %5
      bb2:
        %1 = empty_list [[Event]]
        return %1
    ");
}

#[test]
fn test_lower_division_and_modulo() {
    let result = lower_and_pretty("observer {} /true/ { 100 / 4 + 17 % 5; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 100 [Int]
        %3 = const_int 4 [Int]
        %4 = div %2, %3 [Int]
        %5 = const_int 17 [Int]
        %6 = const_int 5 [Int]
        %7 = mod %5, %6 [Int]
        %8 = add %4, %7 [Int]
        %9 = empty_list [[<error>]]
        return %9
      bb2:
        %1 = empty_list [[Event]]
        return %1
    ");
}

#[test]
fn test_lower_comparison_operators() {
    let result = lower_and_pretty("observer {} /true/ { 1 < 2; 3 <= 3; 5 > 4; 6 >= 6; 1 != 2; 1 == 1; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 1 [Int]
        %3 = const_int 2 [Int]
        %4 = lt %2, %3 [Bool]
        %5 = const_int 3 [Int]
        %6 = const_int 3 [Int]
        %7 = le %5, %6 [Bool]
        %8 = const_int 5 [Int]
        %9 = const_int 4 [Int]
        %10 = gt %8, %9 [Bool]
        %11 = const_int 6 [Int]
        %12 = const_int 6 [Int]
        %13 = ge %11, %12 [Bool]
        %14 = const_int 1 [Int]
        %15 = const_int 2 [Int]
        %16 = ne %14, %15 [Bool]
        %17 = const_int 1 [Int]
        %18 = const_int 1 [Int]
        %19 = eq %17, %18 [Bool]
        %20 = empty_list [[<error>]]
        return %20
      bb2:
        %1 = empty_list [[Event]]
        return %1
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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 10 [Int]
        %3 = neg %2 [Int]
        %4 = empty_list [[<error>]]
        return %4
      bb2:
        %1 = empty_list [[Event]]
        return %1
    ");
}

#[test]
fn test_lower_not() {
    let result = lower_and_pretty("observer {} /true/ { !true; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_bool true [Bool]
        %3 = not %2 [Bool]
        %4 = empty_list [[<error>]]
        return %4
      bb2:
        %1 = empty_list [[Event]]
        return %1
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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_string "hello" [String]
        %3 = empty_list [[<error>]]
        return %3
      bb2:
        %1 = empty_list [[Event]]
        return %1
    "#);
}

#[test]
fn test_lower_float_literal() {
    let result = lower_and_pretty("observer {} /true/ { 1.5 + 2.5; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_float 1.5 [Float]
        %3 = const_float 2.5 [Float]
        %4 = add %2, %3 [Float]
        %5 = empty_list [[<error>]]
        return %5
      bb2:
        %1 = empty_list [[Event]]
        return %1
    ");
}

#[test]
fn test_lower_unit_literals() {
    let result = lower_and_pretty("observer {} /true/ { 5s; 30min; 25c; 90deg; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_unit 5s [Duration]
        %3 = const_unit 30min [Duration]
        %4 = const_unit 25c [Temperature]
        %5 = const_unit 90deg [Angle]
        %6 = empty_list [[<error>]]
        return %6
      bb2:
        %1 = empty_list [[Event]]
        return %1
    ");
}

// =============================================================================
// Field access
// =============================================================================

#[test]
fn test_lower_field_access() {
    let src = r#"observer {
  state = { lights, ... },
  ...
} /true/ {
  lights;
  []
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      Params:
        %0: state [State]
      bb0:
        %1 = field %0.lights [Map<String, LightState>]
        %2 = const_bool true [Bool]
        branch %2 -> bb1, bb2
      bb1:
        %4 = empty_list [[<error>]]
        return %4
      bb2:
        %3 = empty_list [[Event]]
        return %3
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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 1 [Int]
        %3 = const_int 2 [Int]
        %4 = const_int 3 [Int]
        %5 = list [%2, %3, %4] [[Int]]
        %6 = empty_list [[<error>]]
        return %6
      bb2:
        %1 = empty_list [[Event]]
        return %1
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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 42 [Int]
        %3 = empty_list [[<error>]]
        return %3
      bb2:
        %1 = empty_list [[Event]]
        return %1
    ");
}

// =============================================================================
// Nested if/else
// =============================================================================

#[test]
fn test_lower_nested_if() {
    let result = lower_and_pretty("observer {} /true/ { if true { if false { 1 } else { 2 } } else { 3 }; [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %3 = const_bool true [Bool]
        branch %3 -> bb3, bb4
      bb2:
        %1 = empty_list [[Event]]
        return %1
      bb3:
        %5 = const_bool false [Bool]
        branch %5 -> bb6, bb7
      bb4:
        %8 = const_int 3 [Int]
        %2 = copy %8 [Int]
        jump -> bb5
      bb5:
        %9 = empty_list [[<error>]]
        return %9
      bb6:
        %6 = const_int 1 [Int]
        %4 = copy %6 [Int]
        jump -> bb8
      bb7:
        %7 = const_int 2 [Int]
        %4 = copy %7 [Int]
        jump -> bb8
      bb8:
        %2 = copy %4 [Int]
        jump -> bb5
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
      Params:
        %0: event [Event]
      bb0:
        %1 = const_bool true [Bool]
        branch %1 -> bb1, bb2
      bb1:
        return %0
      bb2:
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
      bb0:
        %2 = const_bool true [Bool]
        branch %2 -> bb3, bb4
      bb1:
        %6 = empty_list [[<error>]]
        return %6
      bb2:
        %5 = empty_list [[Event]]
        return %5
      bb3:
        %3 = const_bool false [Bool]
        %1 = copy %3 [Bool]
        jump -> bb5
      bb4:
        %1 = const_bool false [Bool]
        jump -> bb5
      bb5:
        branch %1 -> bb6, bb7
      bb6:
        %0 = const_bool true [Bool]
        jump -> bb8
      bb7:
        %4 = const_bool true [Bool]
        %0 = copy %4 [Bool]
        jump -> bb8
      bb8:
        branch %0 -> bb1, bb2
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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = empty_list [[<error>]]
        return %2
      bb2:
        %1 = empty_list [[Event]]
        return %1
      bb3:
        return %2
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
      bb0:
        %0 = const_bool true [Bool]
        branch %0 -> bb1, bb2
      bb1:
        %2 = const_int 1 [Int]
        %3 = const_int 2 [Int]
        %4 = add %2, %3 [Int]
        %5 = empty_list [[<error>]]
        return %5
      bb2:
        %1 = empty_list [[Event]]
        return %1
    ");
}

// =============================================================================
// List comprehension with filter
// =============================================================================

#[test]
fn test_lower_list_comprehension_with_filter() {
    let src = r#"observer {
  state = { lights, ... },
  ...
} /true/ {
  [ Event::LightStateChanged(l) for l in keys(lights) if true ]
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      Params:
        %0: state [State]
      bb0:
        %1 = field %0.lights [Map<String, LightState>]
        %2 = const_bool true [Bool]
        branch %2 -> bb1, bb2
      bb1:
        %4 = empty_list [[<error>]]
        %5 = call keys(%1) [[String]]
        %6 = iter_init %5 [[String]]
        jump -> bb3
      bb2:
        %3 = empty_list [[Event]]
        return %3
      bb3:
        iter_next %6 -> %7, bb4, bb5
      bb4:
        %9 = const_bool true [Bool]
        branch %9 -> bb6, bb7
      bb5:
        return %4
      bb6:
        %10 = variant Event::LightStateChanged(%7) [Event]
        %11 = list_push %4, %10 [()]
        %12 = unit [()]
        %8 = copy %12 [()]
        jump -> bb8
      bb7:
        %8 = unit [()]
        jump -> bb8
      bb8:
        jump -> bb3
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
    lights,
    binary_sensors,
    ...
  },
  ...
} /true/ {
  lights;
  binary_sensors;
  []
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      Params:
        %0: event [Event]
        %1: state [State]
      bb0:
        %2 = field %1.lights [Map<String, LightState>]
        %3 = field %1.binary_sensors [Map<String, BinarySensorState>]
        %4 = const_bool true [Bool]
        branch %4 -> bb1, bb2
      bb1:
        %6 = empty_list [[<error>]]
        return %6
      bb2:
        %5 = empty_list [[Event]]
        return %5
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
    lights,
    ...
  },
  ...
} /true/ {
  [ Event::LightStateChanged(l) for l in keys(lights) ]
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      Params:
        %0: event [Event]
        %1: state [State]
      bb0:
        %2 = field %1.lights [Map<String, LightState>]
        %3 = const_bool true [Bool]
        branch %3 -> bb1, bb2
      bb1:
        %5 = empty_list [[<error>]]
        %6 = call keys(%2) [[String]]
        %7 = iter_init %6 [[String]]
        jump -> bb3
      bb2:
        %4 = empty_list [[Event]]
        return %4
      bb3:
        iter_next %7 -> %8, bb4, bb5
      bb4:
        %9 = variant Event::LightStateChanged(%8) [Event]
        %10 = list_push %5, %9 [()]
        jump -> bb3
      bb5:
        return %5
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
      Params:
        %0: event [Event]
      bb0:
        %1 = const_bool true [Bool]
        branch %1 -> bb1, bb2
      bb1:
        %2 = const_int 100 [Int]
        %3 = const_int 2 [Int]
        %4 = mul %2, %3 [Int]
        %5 = const_int 0 [Int]
        %6 = const_int 255 [Int]
        %7 = call clamp(%4, %5, %6) [Int]
        %8 = struct Event { brightness: %7, ...%0 } [Event]
        return %8
      bb2:
        return %0
    ");
}

#[test]
fn test_lower_no_filter() {
    let result = lower_and_pretty("observer {} { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      bb0:
        %0 = empty_list [[<error>]]
        return %0
    ");
}

#[test]
fn test_lower_observer_if_else_with_events() {
    let src = r#"observer {
  event,
  state = { lights, ... },
  ...
} /true/ {
  if true {
    [ Event::LightStateChanged(l) for l in keys(lights) ]
  } else {
    []
  }
}"#;
    let result = lower_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      Params:
        %0: event [Event]
        %1: state [State]
      bb0:
        %2 = field %1.lights [Map<String, LightState>]
        %3 = const_bool true [Bool]
        branch %3 -> bb1, bb2
      bb1:
        %6 = const_bool true [Bool]
        branch %6 -> bb3, bb4
      bb2:
        %4 = empty_list [[Event]]
        return %4
      bb3:
        %7 = empty_list [[<error>]]
        %8 = call keys(%2) [[String]]
        %9 = iter_init %8 [[String]]
        jump -> bb6
      bb4:
        %13 = empty_list [[<error>]]
        %5 = copy %13 [[Event]]
        jump -> bb5
      bb5:
        return %5
      bb6:
        iter_next %9 -> %10, bb7, bb8
      bb7:
        %11 = variant Event::LightStateChanged(%10) [Event]
        %12 = list_push %7, %11 [()]
        jump -> bb6
      bb8:
        %5 = copy %7 [[Event]]
        jump -> bb5
    ");
}
