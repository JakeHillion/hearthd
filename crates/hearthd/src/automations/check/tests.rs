use super::check_program;
use super::format_type_errors;
use crate::automations::pretty_print::PrettyPrint;

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
    let result = check_and_pretty("observer { state = { nodes, ... }, ... } /true/ { nodes }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: state
            PatternStruct:
              FieldPattern: nodes
              Rest: ...
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Ident: nodes [type: Map<NodeId, Node>]
    Errors:
      type error at 50..55: observer body must return [Event], found Map<NodeId, Node>
    ");
}

#[test]
fn test_check_pattern_with_two_fields() {
    let result =
        check_and_pretty("observer { state = { nodes, by_entity_id, ... }, ... } /true/ { nodes }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: state
            PatternStruct:
              FieldPattern: nodes
              FieldPattern: by_entity_id
              Rest: ...
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Ident: nodes [type: Map<NodeId, Node>]
    Errors:
      type error at 64..69: observer body must return [Event], found Map<NodeId, Node>
    ");
}

// =============================================================================
// Field access and entity constraints
// =============================================================================

#[test]
fn test_check_field_access() {
    let result = check_and_pretty("observer { state, ... } /true/ { state.nodes }");
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
          Field: .nodes [type: Map<NodeId, Node>]
            Ident: state [type: State]
    Errors:
      type error at 33..44: observer body must return [Event], found Map<NodeId, Node>
    ");
}

// =============================================================================
// Path resolution (enum variants)
// =============================================================================

#[test]
fn test_check_enum_path() {
    let result = check_and_pretty("observer {} { Event::OnOffChanged }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Path: [type: Event::OnOffChanged]
            Segment: Event
            Segment: OnOffChanged
    Errors:
      type error at 14..33: observer body must return [Event], found Event::OnOffChanged
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
        check_and_pretty("observer { state = { nodes, ... }, ... } /true/ { keys(nodes) }");
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: state
            PatternStruct:
              FieldPattern: nodes
              Rest: ...
          Rest: ...
      Filter:
        Bool: true [type: Bool]
      Body:
        ExprStmt:
          Call: [type: [NodeId]]
            Ident: keys [builtin]
            Args:
              Ident: nodes [type: Map<NodeId, Node>]
    Errors:
      type error at 50..61: observer body must return [Event], found [NodeId]
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
            Ident: sleep [builtin]
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
            Ident: len [builtin]
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
            Ident: clamp [builtin]
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
        "observer { state = { nodes, ... }, ... } /true/ { [Event::OnOffChanged(l) for l in keys(nodes)] }",
    );
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: state
            PatternStruct:
              FieldPattern: nodes
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
                  Call: [type: [NodeId]]
                    Ident: keys [builtin]
                    Args:
                      Ident: nodes [type: Map<NodeId, Node>]
                Body:
                  Push: __result0
                    VariantCtor: Event::OnOffChanged [type: Event]
                      Args:
                        Ident: l [type: NodeId]
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
    nodes,
    ...
  },
  ...
} /true/ {
  [ Event::OnOffChanged(l) for l in keys(nodes) ]
}"#;
    let result = check_and_pretty(src);
    insta::assert_snapshot!(result, @"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: event
          FieldPattern: state
            PatternStruct:
              FieldPattern: nodes
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
                  Call: [type: [NodeId]]
                    Ident: keys [builtin]
                    Args:
                      Ident: nodes [type: Map<NodeId, Node>]
                Body:
                  Push: __result0
                    VariantCtor: Event::OnOffChanged [type: Event]
                      Args:
                        Ident: l [type: NodeId]
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
            Ident: clamp [builtin]
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
fn test_error_await_in_filter() {
    // Filters run synchronously on every event; they must not suspend.
    let result = check_errors("observer {} /await sleep(5s)/ { [] }");
    insta::assert_snapshot!(result, @"
    Error: filter must be Bool, found ()
       ╭─[ <test>:1:14 ]
       │
     1 │ observer {} /await sleep(5s)/ { [] }
       │              ───────┬───────  
       │                     ╰───────── filter must be Bool, found ()
    ───╯
    Error: filter expression cannot use `await`
       ╭─[ <test>:1:14 ]
       │
     1 │ observer {} /await sleep(5s)/ { [] }
       │              ───────┬───────  
       │                     ╰───────── filter expression cannot use `await`
    ───╯
    Error: observer body must return [Event], found [<error>]
       ╭─[ <test>:1:33 ]
       │
     1 │ observer {} /await sleep(5s)/ { [] }
       │                                 ─┬  
       │                                  ╰── observer body must return [Event], found [<error>]
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

/// Struct values are a bag of fields at runtime, so two named types with
/// the same shape would compare equal. `TemperatureMeasurementCluster` and
/// `RelativeHumidityMeasurementCluster` are both a single `measured_value`,
/// which is exactly the pair that would silently hold.
#[test]
fn test_error_struct_equality_is_rejected() {
    let result = check_errors(
        "observer { event, ... } /TemperatureMeasurementCluster { measured_value: 20 } == RelativeHumidityMeasurementCluster { measured_value: 20 }/ { [event] }",
    );
    insta::assert_snapshot!(result, @"
    Error: operator '==' is not supported on TemperatureMeasurementCluster and RelativeHumidityMeasurementCluster: equality is only defined on scalars and collections of scalars
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /TemperatureMeasurementCluster { measured_value: 20 } == RelativeHumidityMeasurementCluster { measured_value: 20 }/ { [event] }
       │                          ────────────────────────────────────────────────────────┬────────────────────────────────────────────────────────  
       │                                                                                  ╰────────────────────────────────────────────────────────── operator '==' is not supported on TemperatureMeasurementCluster and RelativeHumidityMeasurementCluster: equality is only defined on scalars and collections of scalars
    ───╯
    ");
}

/// The restriction follows the value: a struct inside a list compares just
/// as structurally as a bare one.
#[test]
fn test_error_struct_equality_through_list_is_rejected() {
    let result = check_errors(
        "observer { event, ... } /[TemperatureMeasurementCluster { measured_value: 20 }] == [RelativeHumidityMeasurementCluster { measured_value: 20 }]/ { [event] }",
    );
    insta::assert_snapshot!(result, @"
    Error: operator '==' is not supported on [TemperatureMeasurementCluster] and [RelativeHumidityMeasurementCluster]: equality is only defined on scalars and collections of scalars
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /[TemperatureMeasurementCluster { measured_value: 20 }] == [RelativeHumidityMeasurementCluster { measured_value: 20 }]/ { [event] }
       │                          ──────────────────────────────────────────────────────────┬──────────────────────────────────────────────────────────  
       │                                                                                    ╰──────────────────────────────────────────────────────────── operator '==' is not supported on [TemperatureMeasurementCluster] and [RelativeHumidityMeasurementCluster]: equality is only defined on scalars and collections of scalars
    ───╯
    ");
}

/// Enums go with them. Every `Event` variant carries a cluster, so none is
/// comparable today; refusing all named types avoids reasoning about
/// payloads, and equality can come back when something needs it.
#[test]
fn test_error_event_equality_is_rejected() {
    let result = check_errors("observer { event, ... } /event == event/ { [event] }");
    insta::assert_snapshot!(result, @"
    Error: operator '==' is not supported on Event and Event: equality is only defined on scalars and collections of scalars
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /event == event/ { [event] }
       │                          ───────┬──────  
       │                                 ╰──────── operator '==' is not supported on Event and Event: equality is only defined on scalars and collections of scalars
    ───╯
    ");
}

/// A `Future` is a pending computation, not a value: comparing two futures
/// would compare nothing meaningful. Await it first, then compare the result.
#[test]
fn test_error_future_equality_is_rejected() {
    let result = check_errors("observer { event, ... } /sleep(5s) == sleep(5s)/ { [event] }");
    insta::assert_snapshot!(result, @"
    Error: operator '==' is not supported on Future<()> and Future<()>: equality is only defined on scalars and collections of scalars
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /sleep(5s) == sleep(5s)/ { [event] }
       │                          ───────────┬──────────  
       │                                     ╰──────────── operator '==' is not supported on Future<()> and Future<()>: equality is only defined on scalars and collections of scalars
    ───╯
    ");
}

/// Scalars, strings, unit literals and lists of them are unaffected.
#[test]
fn test_equality_on_scalars_still_checks() {
    for src in [
        "observer { event, ... } /1 == 1.0/ { [event] }",
        r#"observer { event, ... } /"a" != "b"/ { [event] }"#,
        "observer { event, ... } /5min == 5min/ { [event] }",
        "observer { event, ... } /[1] == [1.0]/ { [event] }",
        "observer { event, ... } /event.node_id == 1/ { [event] }",
    ] {
        let program = crate::automations::parse(src).expect("source should parse");
        let lowered = crate::automations::desugar_program(program);
        let checked = check_program(&lowered);
        assert!(
            checked.errors.is_empty(),
            "{src} should check cleanly, got {:?}",
            checked.errors,
        );
    }
}

/// Membership is equality against each element, so `in` carries the same
/// restriction — otherwise `x in xs` would answer what `x == xs[0]`
/// refuses to.
#[test]
fn test_error_struct_membership_is_rejected() {
    let result = check_errors(
        "observer { event, ... } /TemperatureMeasurementCluster { measured_value: 20 } in [RelativeHumidityMeasurementCluster { measured_value: 20 }]/ { [event] }",
    );
    insta::assert_snapshot!(result, @"
    Error: 'in' is not supported for TemperatureMeasurementCluster in [RelativeHumidityMeasurementCluster]: membership compares by equality, which is only defined on scalars and collections of scalars
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /TemperatureMeasurementCluster { measured_value: 20 } in [RelativeHumidityMeasurementCluster { measured_value: 20 }]/ { [event] }
       │                          ─────────────────────────────────────────────────────────┬─────────────────────────────────────────────────────────  
       │                                                                                   ╰─────────────────────────────────────────────────────────── 'in' is not supported for TemperatureMeasurementCluster in [RelativeHumidityMeasurementCluster]: membership compares by equality, which is only defined on scalars and collections of scalars
    ───╯
    ");
}

#[test]
fn test_error_event_membership_is_rejected() {
    let result = check_errors("observer { event, ... } /event in [event]/ { [event] }");
    insta::assert_snapshot!(result, @"
    Error: 'in' is not supported for Event in [Event]: membership compares by equality, which is only defined on scalars and collections of scalars
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /event in [event]/ { [event] }
       │                          ────────┬───────  
       │                                  ╰───────── 'in' is not supported for Event in [Event]: membership compares by equality, which is only defined on scalars and collections of scalars
    ───╯
    ");
}

/// Membership over scalars is unaffected.
#[test]
fn test_membership_on_scalars_still_checks() {
    for src in [
        "observer { event, ... } /1 in [1, 2]/ { [event] }",
        "observer { event, ... } /1 in [1.0]/ { [event] }",
        r#"observer { event, ... } /"a" in ["a", "b"]/ { [event] }"#,
        "observer { event, ... } /event.node_id in [1, 7]/ { [event] }",
    ] {
        let program = crate::automations::parse(src).expect("source should parse");
        let lowered = crate::automations::desugar_program(program);
        let checked = check_program(&lowered);
        assert!(
            checked.errors.is_empty(),
            "{src} should check cleanly, got {:?}",
            checked.errors,
        );
    }
}

// =============================================================================
// Numeric operands must be statically typed
// =============================================================================

/// Arithmetic commits to an `Int` or `Float` opcode below the checker, so
/// an operand whose type is merely unknown has nothing to compile to and
/// is reported here rather than deferred to the VM.
///
/// `Event` field access is the one way to reach this without a diagnostic
/// already standing behind it: its typing is deferred, so `event.level`
/// types as an error without being one. Arithmetic on it used to run and
/// answer whatever the runtime value happened to be.
#[test]
fn test_arithmetic_on_an_untyped_operand() {
    let result = check_errors("observer { event, ... } /event.level + 1 > 2/ { [event] }");
    insta::assert_snapshot!(result, @"
    Error: arithmetic operator '+' requires numeric operands, found <error> and Int
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /event.level + 1 > 2/ { [event] }
       │                          ───────┬───────  
       │                                 ╰───────── arithmetic operator '+' requires numeric operands, found <error> and Int
    ───╯
    Error: comparison operator '>' requires numeric operands, found <error> and Int
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /event.level + 1 > 2/ { [event] }
       │                          ─────────┬─────────  
       │                                   ╰─────────── comparison operator '>' requires numeric operands, found <error> and Int
    ───╯
    ");
}

#[test]
fn test_ordering_on_an_untyped_operand() {
    let result = check_errors("observer { event, ... } /event.level < 5/ { [event] }");
    insta::assert_snapshot!(result, @"
    Error: comparison operator '<' requires numeric operands, found <error> and Int
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /event.level < 5/ { [event] }
       │                          ───────┬───────  
       │                                 ╰───────── comparison operator '<' requires numeric operands, found <error> and Int
    ───╯
    ");
}

#[test]
fn test_negation_of_an_untyped_operand() {
    let result = check_errors("observer { event, ... } /-event.level > 0/ { [event] }");
    insta::assert_snapshot!(result, @"
    Error: comparison operator '>' requires numeric operands, found <error> and Int
       ╭─[ <test>:1:26 ]
       │
     1 │ observer { event, ... } /-event.level > 0/ { [event] }
       │                          ────────┬───────  
       │                                  ╰───────── comparison operator '>' requires numeric operands, found <error> and Int
    ───╯
    ");
}

/// A loop variable drawn from a collection with no element type is
/// untyped the same way, so arithmetic on it is rejected too.
#[test]
fn test_arithmetic_on_an_untyped_loop_variable() {
    let result = check_errors("observer { event, ... } /[x + 1 for x in []] == []/ { [event] }");
    insta::assert_snapshot!(result, @"
    Error: arithmetic operator '+' requires numeric operands, found <error> and Int
       ╭─[ <test>:1:27 ]
       │
     1 │ observer { event, ... } /[x + 1 for x in []] == []/ { [event] }
       │                           ──┬──  
       │                             ╰──── arithmetic operator '+' requires numeric operands, found <error> and Int
    ───╯
    ");
}

/// Equality and membership still accept an untyped operand: they stay
/// polymorphic, so there is no overload to choose and nothing to reject.
#[test]
fn test_equality_on_an_untyped_operand_still_checks() {
    for src in [
        "observer { event, ... } /event.level == 1/ { [event] }",
        "observer { event, ... } /event.level in [1, 2]/ { [event] }",
        "observer { event, ... } /[x for x in []] == []/ { [event] }",
    ] {
        let program = crate::automations::parse(src).expect("source should parse");
        let lowered = crate::automations::desugar_program(program);
        let checked = check_program(&lowered);
        assert!(
            checked.errors.is_empty(),
            "{src} should check cleanly, got {:?}",
            checked.errors,
        );
    }
}
// =============================================================================
// Structural state binding (state.<domain>.<slug>)
// =============================================================================

/// Naming an entity type checks with no deployment in sight. `state.light`
/// is the light domain on every install, and any slug under it is a `Node`,
/// so the automation compiles before anything knows whether this house has
/// a living room lamp.
#[test]
fn test_state_domain_slug_resolves_to_node() {
    let result = check_errors(
        r#"observer { event, state, ... } /state.light.living_room_lamp.entity_id == "light.living_room_lamp"/ { [event] }"#,
    );
    insta::assert_snapshot!(result, @"");
}

/// The same path written as a destructuring pattern, which resolves through
/// the same rule: a field on a domain group names an entity either way.
#[test]
fn test_state_domain_slug_destructures() {
    let result = check_errors(
        r#"observer { event, state = { light = { living_room_lamp }, ... }, ... } /living_room_lamp.entity_id == "x"/ { [event] }"#,
    );
    insta::assert_snapshot!(result, @"");
}

/// A domain the language has no variant for is an ordinary unknown field.
/// This is settled by the `Domain` enum rather than by any deployment, so
/// the typo is caught on a house that has no lights at all.
#[test]
fn test_unknown_domain_on_state_is_a_type_error() {
    let result = check_errors(
        r#"observer { event, state, ... } /state.lite.living_room_lamp.entity_id == "x"/ { [event] }"#,
    );
    insta::assert_snapshot!(result, @r#"
    Error: no field 'lite' on type State
       ╭─[ <test>:1:33 ]
       │
     1 │ observer { event, state, ... } /state.lite.living_room_lamp.entity_id == "x"/ { [event] }
       │                                 ─────┬────  
       │                                      ╰────── no field 'lite' on type State
    ───╯
    "#);
}

/// Domains are laid over the facet shape rather than replacing it, so the
/// raw maps `state` has always had are still reachable.
#[test]
fn test_state_keeps_its_facet_fields() {
    let result = check_errors(
        r#"observer { event, state = { nodes, light = { living_room_lamp }, ... }, ... } /living_room_lamp.entity_id == "x"/ { [event] }"#,
    );
    insta::assert_snapshot!(result, @"");
}

/// A domain group is a value in its own right: it can be bound and passed
/// around without naming any entity, and doing so records no symbol. This is
/// the case that cannot fail to relocate — a house with no lights runs it
/// and gets nothing.
#[test]
fn test_domain_group_binds_without_naming_an_entity() {
    let result = check_errors(
        r#"observer { event, state, ... } /true/ { let lights = state.light; [event] }"#,
    );
    insta::assert_snapshot!(result, @"");
}

/// Two domains are two types, and there is no join between them. The
/// language has no tagged unions, so a value that might be either has
/// nothing to be -- and a field on it names no entity, which is what makes
/// this a safety property rather than a nicety: a silent join would emit a
/// symbol for whichever branch won and relocate to a device the source
/// never named.
///
/// The diagnostic is poor. `unify` cannot report, so the mismatch poisons
/// to `<error>` and the complaint surfaces wherever the value is used.
/// Rejecting the program is the part that matters here.
#[test]
fn test_domain_groups_of_different_domains_do_not_unify() {
    let result = check_errors(
        r#"observer { event, state, ... } /true/ { let g = if true { state.light } else { state.climate }; [g.living_room_lamp] }"#,
    );
    insta::assert_snapshot!(result, @r#"
    Error: observer body must return [Event], found [<error>]
       ╭─[ <test>:1:97 ]
       │
     1 │ observer { event, state, ... } /true/ { let g = if true { state.light } else { state.climate }; [g.living_room_lamp] }
       │                                                                                                 ──────────┬─────────  
       │                                                                                                           ╰─────────── observer body must return [Event], found [<error>]
    ───╯
    "#);
}

// =============================================================================
// Entity symbols
// =============================================================================

/// The symbols a source names, as `domain.slug`, in the order recorded.
fn entity_symbols(input: &str) -> Vec<String> {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    let result = check_program(&lowered);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    result
        .constraints
        .iter()
        .map(|c| format!("{}.{}", c.domain, c.entity))
        .collect()
}

/// Naming an entity records it, which is the whole output the relocator
/// later consumes.
#[test]
fn test_named_entity_is_recorded_as_a_symbol() {
    let symbols = entity_symbols(
        r#"observer { event, state, ... } /state.light.living_room_lamp.entity_id == "x"/ { [event] }"#,
    );
    insta::assert_debug_snapshot!(symbols, @r#"
    [
        "light.living_room_lamp",
    ]
    "#);
}

/// A destructuring pattern records the same symbol the path does.
#[test]
fn test_destructured_entity_is_recorded_as_a_symbol() {
    let symbols = entity_symbols(
        r#"observer { event, state = { light = { living_room_lamp }, ... }, ... } /living_room_lamp.entity_id == "x"/ { [event] }"#,
    );
    insta::assert_debug_snapshot!(symbols, @r#"
    [
        "light.living_room_lamp",
    ]
    "#);
}

/// Binding a domain group without naming anything under it records nothing:
/// there is no entity here for a deployment to be missing.
#[test]
fn test_domain_group_alone_records_no_symbol() {
    let symbols = entity_symbols(
        r#"observer { event, state, ... } /true/ { let lights = state.light; [event] }"#,
    );
    insta::assert_debug_snapshot!(symbols, @"[]");
}

/// The symbols recorded by a source the checker rejected.
///
/// A rejected program never reaches the relocator, but a symbol emitted
/// here would mean the checker resolved a name the source did not settle.
fn entity_symbols_allowing_errors(input: &str) -> Vec<String> {
    let program = crate::automations::parse(input).expect("parsing should succeed");
    let lowered = crate::automations::desugar_program(program);
    check_program(&lowered)
        .constraints
        .iter()
        .map(|c| format!("{}.{}", c.domain, c.entity))
        .collect()
}

/// A field on a value that could be either of two domains records nothing.
/// Picking the first branch's domain would name a device the source never
/// wrote, and relocation would resolve it happily.
#[test]
fn test_mismatched_domain_groups_record_no_symbol() {
    let symbols = entity_symbols_allowing_errors(
        r#"observer { event, state, ... } /true/ { let g = if true { state.light } else { state.climate }; [g.living_room_lamp] }"#,
    );
    insta::assert_debug_snapshot!(symbols, @"[]");
}
