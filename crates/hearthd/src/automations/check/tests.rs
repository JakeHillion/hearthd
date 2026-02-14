use super::check_program;
use super::format_type_errors;
use crate::automations::repr::pretty_print::PrettyPrint;

fn check_and_pretty(input: &str) -> String {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    let result = check_program(&lowered);
    result.to_pretty_string()
}

/// Strip ANSI escape sequences so snapshot output is stable and readable.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until 'm' (SGR terminator) or end of string
            for c2 in chars.by_ref() {
                if c2 == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse, desugar, check, and render errors with ariadne (ANSI stripped).
fn check_errors(input: &str) -> String {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    let result = check_program(&lowered);
    let rendered = format_type_errors(&result.errors, input, "<test>");
    strip_ansi(&rendered)
}

// =============================================================================
// Literal type checking
// =============================================================================

#[test]
fn test_check_observer_int_literal() {
    let result = check_and_pretty("observer {} { 42 }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Int: 42 [type: Int]
    Errors:
      type error at 14..16: observer body must return [Event], found Int
    ");
}

#[test]
fn test_check_observer_bool_literal() {
    let result = check_and_pretty("observer {} { true }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Bool: true [type: Bool]
    Errors:
      type error at 14..18: observer body must return [Event], found Bool
    ");
}

#[test]
fn test_check_observer_string_literal() {
    let result = check_and_pretty(r#"observer {} { "hello" }"#);
    insta::assert_snapshot!(result, @r#"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          String: "hello" [type: String]
    Errors:
      type error at 14..21: observer body must return [Event], found String
    "#);
}

#[test]
fn test_check_observer_float_literal() {
    let result = check_and_pretty("observer {} { 3.14 }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Float: 3.14 [type: Float]
    Errors:
      type error at 14..18: observer body must return [Event], found Float
    ");
}

#[test]
fn test_check_observer_unit_literal_duration() {
    let result = check_and_pretty("observer {} { 5min }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          UnitLiteral: 5min [type: Duration]
    Errors:
      type error at 14..18: observer body must return [Event], found Duration
    ");
}

#[test]
fn test_check_observer_unit_literal_angle() {
    let result = check_and_pretty("observer {} { 90deg }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          UnitLiteral: 90deg [type: Angle]
    Errors:
      type error at 14..19: observer body must return [Event], found Angle
    ");
}

#[test]
fn test_check_observer_unit_literal_temperature() {
    let result = check_and_pretty("observer {} { 20c }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          UnitLiteral: 20c [type: Temperature]
    Errors:
      type error at 14..17: observer body must return [Event], found Temperature
    ");
}

// =============================================================================
// Variable binding and lookup
// =============================================================================

#[test]
fn test_check_let_binding() {
    let result = check_and_pretty("observer {} { let x = 42; x }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        Let: x
          Int: 42 [type: Int]
        ExprStmt:
          Ident: x [type: Int]
    Errors:
      type error at 26..27: observer body must return [Event], found Int
    ");
}

#[test]
fn test_check_undefined_variable() {
    let result = check_and_pretty("observer {} { unknown }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Ident: unknown [type: <error>]
    Errors:
      type error at 14..21: undefined variable 'unknown'
    ");
}

// =============================================================================
// Binary operations
// =============================================================================

#[test]
fn test_check_arithmetic() {
    let result = check_and_pretty("observer {} { let x = 1 + 2; x }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        Let: x
          BinOp: + [type: Int]
            Int: 1 [type: Int]
            Int: 2 [type: Int]
        ExprStmt:
          Ident: x [type: Int]
    Errors:
      type error at 29..30: observer body must return [Event], found Int
    ");
}

#[test]
fn test_check_comparison() {
    let result = check_and_pretty("observer {} { 1 > 2 }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          BinOp: > [type: Bool]
            Int: 1 [type: Int]
            Int: 2 [type: Int]
    Errors:
      type error at 14..19: observer body must return [Event], found Bool
    ");
}

#[test]
fn test_check_logical() {
    let result = check_and_pretty("observer {} { true && false }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          BinOp: && [type: Bool]
            Bool: true [type: Bool]
            Bool: false [type: Bool]
    Errors:
      type error at 14..27: observer body must return [Event], found Bool
    ");
}

#[test]
fn test_check_equality() {
    let result = check_and_pretty("observer {} { 1 == 2 }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          BinOp: == [type: Bool]
            Int: 1 [type: Int]
            Int: 2 [type: Int]
    Errors:
      type error at 14..20: observer body must return [Event], found Bool
    ");
}

#[test]
fn test_check_arithmetic_type_error() {
    let result = check_and_pretty(r#"observer {} { "hello" + 1 }"#);
    insta::assert_snapshot!(result, @r#"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          BinOp: + [type: <error>]
            String: "hello" [type: String]
            Int: 1 [type: Int]
    Errors:
      type error at 14..25: arithmetic operator '+' requires numeric operands, found String and Int
    "#);
}

#[test]
fn test_check_float_contamination() {
    let result = check_and_pretty("observer {} { 1 + 3.14 }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          BinOp: + [type: Float]
            Int: 1 [type: Int]
            Float: 3.14 [type: Float]
    Errors:
      type error at 14..22: observer body must return [Event], found Float
    ");
}

// =============================================================================
// Pattern destructuring
// =============================================================================

#[test]
fn test_check_pattern_simple() {
    let result = check_and_pretty("observer { event, ... } /true/ { event }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: event
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Ident: event [type: Event]
    Errors:
      type error at 33..38: observer body must return [Event], found Event
    ");
}

#[test]
fn test_check_pattern_nested() {
    let result = check_and_pretty("observer { state = { lights, ... }, ... } /true/ { lights }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: state
            PatternStruct:
              FieldPattern: lights
              Rest: ...
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Ident: lights [type: Map<String, LightState>]
    Errors:
      type error at 51..57: observer body must return [Event], found Map<String, LightState>
    ");
}

#[test]
fn test_check_pattern_with_two_fields() {
    let result = check_and_pretty(
        "observer { state = { lights, binary_sensors, ... }, ... } /true/ { lights }",
    );
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: state
            PatternStruct:
              FieldPattern: lights
              FieldPattern: binary_sensors
              Rest: ...
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Ident: lights [type: Map<String, LightState>]
    Errors:
      type error at 67..73: observer body must return [Event], found Map<String, LightState>
    ");
}

// =============================================================================
// Field access and entity constraints
// =============================================================================

#[test]
fn test_check_field_access() {
    let result = check_and_pretty("observer { state, ... } /true/ { state.lights }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: state
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Field: .lights [type: Map<String, LightState>]
            Ident: state [type: State]
    Errors:
      type error at 33..45: observer body must return [Event], found Map<String, LightState>
    ");
}

// =============================================================================
// Path resolution (enum variants)
// =============================================================================

#[test]
fn test_check_enum_path() {
    let result = check_and_pretty("observer {} { Event::LightStateChanged }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Path: [type: Event::LightStateChanged]
            Segment: Event
            Segment: LightStateChanged
    Errors:
      type error at 14..38: observer body must return [Event], found Event::LightStateChanged
    ");
}

#[test]
fn test_check_unknown_enum_variant() {
    let result = check_and_pretty("observer {} { Event::Unknown }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Path: [type: <error>]
            Segment: Event
            Segment: Unknown
    Errors:
      type error at 14..28: unknown variant 'Unknown' on enum 'Event'
    ");
}

// =============================================================================
// Built-in function calls
// =============================================================================

#[test]
fn test_check_builtin_keys() {
    let result =
        check_and_pretty("observer { state = { lights, ... }, ... } /true/ { keys(lights) }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: state
            PatternStruct:
              FieldPattern: lights
              Rest: ...
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Call: [type: [String]]
            Ident: keys [type: <error>]
            Args:
              Ident: lights [type: Map<String, LightState>]
    Errors:
      type error at 51..63: observer body must return [Event], found [String]
    ");
}

#[test]
fn test_check_builtin_sleep() {
    let result = check_and_pretty("observer {} { sleep(5min) }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Call: [type: Future<()>]
            Ident: sleep [type: <error>]
            Args:
              UnitLiteral: 5min [type: Duration]
    Errors:
      type error at 14..25: observer body must return [Event], found Future<()>
    ");
}

#[test]
fn test_check_builtin_len() {
    let result = check_and_pretty(r#"observer {} { len("hello") }"#);
    insta::assert_snapshot!(result, @r#"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Call: [type: Int]
            Ident: len [type: <error>]
            Args:
              String: "hello" [type: String]
    Errors:
      type error at 14..26: observer body must return [Event], found Int
    "#);
}

#[test]
fn test_check_builtin_clamp() {
    let result = check_and_pretty("observer {} { clamp(50, 0, 100) }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Call: [type: Int]
            Ident: clamp [type: <error>]
            Args:
              Int: 50 [type: Int]
              Int: 0 [type: Int]
              Int: 100 [type: Int]
    Errors:
      type error at 14..31: observer body must return [Event], found Int
    ");
}

// =============================================================================
// Control flow
// =============================================================================

#[test]
fn test_check_if_else() {
    let result = check_and_pretty("observer {} { if true { 1 } else { 2 } }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          If: [type: Int]
            Cond:
              Bool: true [type: Bool]
            Then:
              ExprStmt:
                Int: 1 [type: Int]
            Else:
              ExprStmt:
                Int: 2 [type: Int]
    Errors:
      type error at 14..38: observer body must return [Event], found Int
    ");
}

#[test]
fn test_check_if_without_else() {
    let result = check_and_pretty("observer {} { if true { 1 } }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          If: [type: ()]
            Cond:
              Bool: true [type: Bool]
            Then:
              ExprStmt:
                Int: 1 [type: Int]
    ");
}

// =============================================================================
// List comprehensions (desugared)
// =============================================================================

#[test]
fn test_check_list_comp() {
    let result = check_and_pretty(
        "observer { state = { lights, ... }, ... } /true/ { [Event::LightStateChanged(l) for l in keys(lights)] }",
    );
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: state
            PatternStruct:
              FieldPattern: lights
              Rest: ...
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Block: [type: [Event]]
            Stmts:
              LetMut: __result0
                MutableList [type: [<error>]]
              For:
                Var: l
                Iter:
                  Call: [type: [String]]
                    Ident: keys [type: <error>]
                    Args:
                      Ident: lights [type: Map<String, LightState>]
                Body:
                  Push: __result0
                    Call: [type: Event]
                      Path: [type: Event::LightStateChanged]
                        Segment: Event
                        Segment: LightStateChanged
                      Args:
                        Ident: l [type: String]
            Result:
              Ident: __result0 [type: [Event]]
    ");
}

// =============================================================================
// Struct literals
// =============================================================================

#[test]
fn test_check_struct_literal() {
    let result = check_and_pretty("observer {} { Event { device: \"lamp\" } }");
    insta::assert_snapshot!(result, @r#"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          StructLit: Event [type: Event]
            Field: device
              String: "lamp" [type: String]
    Errors:
      type error at 14..38: observer body must return [Event], found Event
    "#);
}

// =============================================================================
// Return type validation
// =============================================================================

#[test]
fn test_check_mutator_return_type() {
    let result = check_and_pretty(r#"mutator { event, ... } /true/ { Event { device: "lamp" } }"#);
    insta::assert_snapshot!(result, @r#"
    Automation: mutator
      Pattern:
        PatternStruct:
          FieldPattern: event
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          StructLit: Event [type: Event]
            Field: device
              String: "lamp" [type: String]
    "#);
}

// =============================================================================
// Filter checking
// =============================================================================

#[test]
fn test_check_filter_bool() {
    let result = check_and_pretty("observer { event, ... } /true/ { [] }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: event
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          List: (empty) [type: [<error>]]
    Errors:
      type error at 33..35: observer body must return [Event], found [<error>]
    ");
}

// =============================================================================
// Unary operations
// =============================================================================

#[test]
fn test_check_negation() {
    let result = check_and_pretty("observer {} { -42 }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          UnaryOp: - [type: Int]
            Int: 42 [type: Int]
    Errors:
      type error at 14..17: observer body must return [Event], found Int
    ");
}

#[test]
fn test_check_logical_not() {
    let result = check_and_pretty("observer {} { !true }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          UnaryOp: ! [type: Bool]
            Bool: true [type: Bool]
    Errors:
      type error at 14..19: observer body must return [Event], found Bool
    ");
}

// =============================================================================
// Integration: design doc example
// =============================================================================

#[test]
fn test_check_lights_off_automation() {
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
    let result = check_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: event
          FieldPattern: state
            PatternStruct:
              FieldPattern: lights
              Rest: ...
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Block: [type: [Event]]
            Stmts:
              LetMut: __result0
                MutableList [type: [<error>]]
              For:
                Var: l
                Iter:
                  Call: [type: [String]]
                    Ident: keys [type: <error>]
                    Args:
                      Ident: lights [type: Map<String, LightState>]
                Body:
                  Push: __result0
                    Call: [type: Event]
                      Path: [type: Event::LightStateChanged]
                        Segment: Event
                        Segment: LightStateChanged
                      Args:
                        Ident: l [type: String]
            Result:
              Ident: __result0 [type: [Event]]
    ");
}

#[test]
fn test_check_mutator_with_computation() {
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
    let result = check_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: mutator
      Pattern:
        PatternStruct:
          FieldPattern: event
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        Let: brightness
          Call: [type: Int]
            Ident: clamp [type: <error>]
            Args:
              BinOp: * [type: Int]
                Int: 100 [type: Int]
                Int: 2 [type: Int]
              Int: 0 [type: Int]
              Int: 255 [type: Int]
        ExprStmt:
          StructLit: Event [type: Event]
            Inherit: brightness
            Spread: event
    ");
}

// =============================================================================
// Error rendering tests (ariadne pretty output)
// =============================================================================

#[test]
fn test_error_arithmetic_on_strings() {
    let result = check_errors(r#"observer {} { "hello" + 1 }"#);
    insta::assert_snapshot!(result, @r#"
    Error: arithmetic operator '+' requires numeric operands, found String and Int
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { "hello" + 1 }
       │               ─────┬─────  
       │                    ╰─────── arithmetic operator '+' requires numeric operands, found String and Int
    ───╯
    "#);
}

#[test]
fn test_error_comparison_on_strings() {
    let result = check_errors(r#"observer {} { "a" > "b" }"#);
    insta::assert_snapshot!(result, @r#"
    Error: comparison operator '>' requires numeric operands, found String and String
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { "a" > "b" }
       │               ────┬────  
       │                   ╰────── comparison operator '>' requires numeric operands, found String and String
    ───╯
    "#);
}

#[test]
fn test_error_logical_on_int() {
    let result = check_errors("observer {} { 1 && 2 }");
    insta::assert_snapshot!(result, @"
    Error: logical operator '&&' requires Bool operands, found Int and Int
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { 1 && 2 }
       │               ───┬──  
       │                  ╰──── logical operator '&&' requires Bool operands, found Int and Int
    ───╯
    ");
}

#[test]
fn test_error_negation_on_bool() {
    let result = check_errors("observer {} { -true }");
    insta::assert_snapshot!(result, @"
    Error: negation requires numeric type, found Bool
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { -true }
       │               ──┬──  
       │                 ╰──── negation requires numeric type, found Bool
    ───╯
    ");
}

#[test]
fn test_error_not_on_int() {
    let result = check_errors("observer {} { !42 }");
    insta::assert_snapshot!(result, @"
    Error: logical not requires Bool, found Int
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { !42 }
       │               ─┬─  
       │                ╰─── logical not requires Bool, found Int
    ───╯
    ");
}

#[test]
fn test_error_await_on_int() {
    let result = check_errors("observer {} { await 42 }");
    insta::assert_snapshot!(result, @"
    Error: await requires Future type, found Int
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { await 42 }
       │               ────┬───  
       │                   ╰───── await requires Future type, found Int
    ───╯
    ");
}

#[test]
fn test_error_in_on_non_collection() {
    let result = check_errors("observer {} { 1 in 2 }");
    insta::assert_snapshot!(result, @"
    Error: 'in' requires collection on right side, found Int
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { 1 in 2 }
       │               ───┬──  
       │                  ╰──── 'in' requires collection on right side, found Int
    ───╯
    ");
}

#[test]
fn test_error_undefined_variable() {
    let result = check_errors("observer {} { unknown }");
    insta::assert_snapshot!(result, @"
    Error: undefined variable 'unknown'
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { unknown }
       │               ───┬───  
       │                  ╰───── undefined variable 'unknown'
    ───╯
    ");
}

#[test]
fn test_error_unknown_enum_variant() {
    let result = check_errors("observer {} { Event::Nope }");
    insta::assert_snapshot!(result, @"
    Error: unknown variant 'Nope' on enum 'Event'
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { Event::Nope }
       │               ─────┬─────  
       │                    ╰─────── unknown variant 'Nope' on enum 'Event'
    ───╯
    ");
}

#[test]
fn test_error_unknown_type_path() {
    let result = check_errors("observer {} { Foo::Bar }");
    insta::assert_snapshot!(result, @"
    Error: unknown type 'Foo'
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { Foo::Bar }
       │               ────┬───  
       │                   ╰───── unknown type 'Foo'
    ───╯
    ");
}

#[test]
fn test_error_if_cond_not_bool() {
    let result = check_errors("observer {} { if 42 { 1 } }");
    insta::assert_snapshot!(result, @"
    Error: if condition must be Bool, found Int
       ╭─[ <test>:1:18 ]
       │
     1 │ observer {} { if 42 { 1 } }
       │                  ─┬  
       │                   ╰── if condition must be Bool, found Int
    ───╯
    ");
}

#[test]
fn test_error_filter_not_bool() {
    let result = check_errors("observer {} /42/ { [] }");
    insta::assert_snapshot!(result, @"
    Error: filter must be Bool, found Int
       ╭─[ <test>:1:14 ]
       │
     1 │ observer {} /42/ { [] }
       │              ─┬  
       │               ╰── filter must be Bool, found Int
    ───╯
    Error: observer body must return [Event], found [<error>]
       ╭─[ <test>:1:20 ]
       │
     1 │ observer {} /42/ { [] }
       │                    ─┬  
       │                     ╰── observer body must return [Event], found [<error>]
    ───╯
    ");
}

#[test]
fn test_error_observer_wrong_return() {
    let result = check_errors("observer {} { 42 }");
    insta::assert_snapshot!(result, @"
    Error: observer body must return [Event], found Int
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { 42 }
       │               ─┬  
       │                ╰── observer body must return [Event], found Int
    ───╯
    ");
}

#[test]
fn test_error_mutator_wrong_return() {
    let result = check_errors("mutator {} { [] }");
    insta::assert_snapshot!(result, @"
    Error: mutator body must return Event, found [<error>]
       ╭─[ <test>:1:14 ]
       │
     1 │ mutator {} { [] }
       │              ─┬  
       │               ╰── mutator body must return Event, found [<error>]
    ───╯
    ");
}

#[test]
fn test_error_unknown_field() {
    let result = check_errors("observer { state, ... } /true/ { state.nonexistent }");
    insta::assert_snapshot!(result, @"
    Error: no field 'nonexistent' on type State
       ╭─[ <test>:1:34 ]
       │
     1 │ observer { state, ... } /true/ { state.nonexistent }
       │                                  ────────┬────────  
       │                                          ╰────────── no field 'nonexistent' on type State
    ───╯
    ");
}

#[test]
fn test_error_sleep_wrong_arg() {
    let result = check_errors("observer {} { sleep(42) }");
    insta::assert_snapshot!(result, @"
    Error: sleep() requires Duration, found Int
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { sleep(42) }
       │               ────┬────  
       │                   ╰────── sleep() requires Duration, found Int
    ───╯
    Error: observer body must return [Event], found Future<()>
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { sleep(42) }
       │               ────┬────  
       │                   ╰────── observer body must return [Event], found Future<()>
    ───╯
    ");
}

#[test]
fn test_error_keys_on_non_map() {
    let result = check_errors("observer {} { keys(42) }");
    insta::assert_snapshot!(result, @"
    Error: keys() requires Map, found Int
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { keys(42) }
       │               ────┬───  
       │                   ╰───── keys() requires Map, found Int
    ───╯
    ");
}

#[test]
fn test_error_unknown_function() {
    let result = check_errors("observer {} { foo(1) }");
    insta::assert_snapshot!(result, @"
    Error: undefined function 'foo'
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { foo(1) }
       │               ───┬──  
       │                  ╰──── undefined function 'foo'
    ───╯
    ");
}

#[test]
fn test_error_unknown_struct() {
    let result = check_errors(r#"observer {} { Foo { x: 1 } }"#);
    insta::assert_snapshot!(result, @"
    Error: unknown struct type 'Foo'
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { Foo { x: 1 } }
       │               ──────┬─────  
       │                     ╰─────── unknown struct type 'Foo'
    ───╯
    ");
}

#[test]
fn test_error_for_non_iterable() {
    let result = check_errors("observer {} { [x for x in 42] }");
    insta::assert_snapshot!(result, @"
    Error: cannot iterate over Int
       ╭─[ <test>:1:27 ]
       │
     1 │ observer {} { [x for x in 42] }
       │                           ─┬  
       │                            ╰── cannot iterate over Int
    ───╯
    Error: observer body must return [Event], found [<error>]
       ╭─[ <test>:1:15 ]
       │
     1 │ observer {} { [x for x in 42] }
       │               ───────┬───────  
       │                      ╰───────── observer body must return [Event], found [<error>]
    ───╯
    ");
}

#[test]
fn test_error_multiple_errors() {
    let result = check_errors(r#"observer {} { let a = "hi" + 1; !42 }"#);
    insta::assert_snapshot!(result, @r#"
    Error: arithmetic operator '+' requires numeric operands, found String and Int
       ╭─[ <test>:1:23 ]
       │
     1 │ observer {} { let a = "hi" + 1; !42 }
       │                       ────┬───  
       │                           ╰───── arithmetic operator '+' requires numeric operands, found String and Int
    ───╯
    Error: logical not requires Bool, found Int
       ╭─[ <test>:1:33 ]
       │
     1 │ observer {} { let a = "hi" + 1; !42 }
       │                                 ─┬─  
       │                                  ╰─── logical not requires Bool, found Int
    ───╯
    "#);
}
