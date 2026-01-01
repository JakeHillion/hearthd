use chumsky::prelude::*;

use super::expr_parser;
use crate::automations::ast::*;
use crate::automations::lexer::Token;
use crate::automations::pretty_print::PrettyPrint;

fn parse_expr(input: &str) -> Result<Spanned<Expr>, Vec<Rich<'static, Token>>> {
    let tokens = crate::automations::lexer::lexer()
        .parse(input)
        .into_result()
        .map_err(|errs| {
            errs.into_iter()
                .map(|err| Rich::<Token>::custom(*err.span(), format!("Lexer error: {}", err)))
                .collect::<Vec<_>>()
        })?;
    let input_len = input.len();
    let result = expr_parser()
        .parse(
            tokens
                .as_slice()
                .map((input_len..input_len).into(), |(t, s)| (t, s)),
        )
        .into_result()
        .map_err(|errs| errs.into_iter().map(|e| e.into_owned()).collect());
    result
}

#[test]
fn test_parse_literals() {
    insta::assert_snapshot!(parse_expr("42").unwrap().to_pretty_string(), @r#"
    Int: 42
    "#);
    insta::assert_snapshot!(parse_expr("3.14").unwrap().to_pretty_string(), @r#"
    Float: 3.14
    "#);
    insta::assert_snapshot!(parse_expr("\"hello\"").unwrap().to_pretty_string(), @r#"
    String: "hello"
    "#);
    insta::assert_snapshot!(parse_expr("true").unwrap().to_pretty_string(), @r#"
    Bool: true
    "#);
    insta::assert_snapshot!(parse_expr("false").unwrap().to_pretty_string(), @r#"
    Bool: false
    "#);
}

#[test]
fn test_parse_unit_literals() {
    insta::assert_snapshot!(parse_expr("5min").unwrap().to_pretty_string(), @r#"
    UnitLiteral: 5min
    "#);
    insta::assert_snapshot!(parse_expr("90deg").unwrap().to_pretty_string(), @r#"
    UnitLiteral: 90deg
    "#);
}

#[test]
fn test_parse_identifiers() {
    insta::assert_snapshot!(parse_expr("foo").unwrap().to_pretty_string(), @r#"
    Ident: foo
    "#);
}

#[test]
fn test_parse_binary_ops() {
    insta::assert_snapshot!(parse_expr("1 + 2").unwrap().to_pretty_string(), @r"
    BinOp: +
      Int: 1
      Int: 2
    ");
    // Precedence: multiplication before addition
    insta::assert_snapshot!(parse_expr("1 + 2 * 3").unwrap().to_pretty_string(), @r"
    BinOp: +
      Int: 1
      BinOp: *
        Int: 2
        Int: 3
    ");
    // Left associativity
    insta::assert_snapshot!(parse_expr("1 - 2 - 3").unwrap().to_pretty_string(), @r"
    BinOp: -
      BinOp: -
        Int: 1
        Int: 2
      Int: 3
    ");
}

#[test]
fn test_parse_comparison() {
    insta::assert_snapshot!(parse_expr("x == 5").unwrap().to_pretty_string(), @r"
    BinOp: ==
      Ident: x
      Int: 5
    ");
    insta::assert_snapshot!(parse_expr("a < b").unwrap().to_pretty_string(), @r"
    BinOp: <
      Ident: a
      Ident: b
    ");
    insta::assert_snapshot!(parse_expr("a >= b").unwrap().to_pretty_string(), @r"
    BinOp: >=
      Ident: a
      Ident: b
    ");
    insta::assert_snapshot!(parse_expr("x in collection").unwrap().to_pretty_string(), @r"
    BinOp: in
      Ident: x
      Ident: collection
    ");
    // in binds tighter than &&
    insta::assert_snapshot!(parse_expr("x in list && y").unwrap().to_pretty_string(), @r"
    BinOp: &&
      BinOp: in
        Ident: x
        Ident: list
      Ident: y
    ");
}

#[test]
fn test_parse_logical() {
    insta::assert_snapshot!(parse_expr("true && false").unwrap().to_pretty_string(), @r"
    BinOp: &&
      Bool: true
      Bool: false
    ");
    insta::assert_snapshot!(parse_expr("a || b").unwrap().to_pretty_string(), @r"
    BinOp: ||
      Ident: a
      Ident: b
    ");
    // Precedence: && before ||
    insta::assert_snapshot!(parse_expr("a || b && c").unwrap().to_pretty_string(), @r"
    BinOp: ||
      Ident: a
      BinOp: &&
        Ident: b
        Ident: c
    ");
}

#[test]
fn test_parse_unary_ops() {
    insta::assert_snapshot!(parse_expr("!true").unwrap().to_pretty_string(), @r"
    UnaryOp: !
      Bool: true
    ");
    insta::assert_snapshot!(parse_expr("-42").unwrap().to_pretty_string(), @r"
    UnaryOp: -
      Int: 42
    ");
    insta::assert_snapshot!(parse_expr("*ptr").unwrap().to_pretty_string(), @r"
    UnaryOp: *
      Ident: ptr
    ");
    insta::assert_snapshot!(parse_expr("await future").unwrap().to_pretty_string(), @r"
    UnaryOp: await
      Ident: future
    ");
}

#[test]
fn test_parse_field_access() {
    insta::assert_snapshot!(parse_expr("event.type").unwrap().to_pretty_string(), @r"
    Field: .type
      Ident: event
    ");
    insta::assert_snapshot!(parse_expr("a.b.c").unwrap().to_pretty_string(), @r"
    Field: .c
      Field: .b
        Ident: a
    ");
}

#[test]
fn test_parse_optional_chaining() {
    insta::assert_snapshot!(parse_expr("person?.location").unwrap().to_pretty_string(), @r"
    OptionalField: ?.location
      Ident: person
    ");
    insta::assert_snapshot!(parse_expr("a?.b?.c").unwrap().to_pretty_string(), @r"
    OptionalField: ?.c
      OptionalField: ?.b
        Ident: a
    ");
}

#[test]
fn test_parse_list() {
    insta::assert_snapshot!(parse_expr("[1, 2, 3]").unwrap().to_pretty_string(), @r"
    List:
      Int: 1
      Int: 2
      Int: 3
    ");
    insta::assert_snapshot!(parse_expr("[]").unwrap().to_pretty_string(), @"List: (empty)");
    insta::assert_snapshot!(parse_expr("[a + b, c]").unwrap().to_pretty_string(), @r"
    List:
      BinOp: +
        Ident: a
        Ident: b
      Ident: c
    ");
}

#[test]
fn test_parse_list_comp_basic() {
    insta::assert_snapshot!(parse_expr("[x for x in list]").unwrap().to_pretty_string(), @r"
    ListComp:
      Expr:
        Ident: x
      Var: x
      Iter:
        Ident: list
    ");
}

#[test]
fn test_parse_list_comp_with_expr() {
    insta::assert_snapshot!(parse_expr("[x * 2 for x in items]").unwrap().to_pretty_string(), @r"
    ListComp:
      Expr:
        BinOp: *
          Ident: x
          Int: 2
      Var: x
      Iter:
        Ident: items
    ");
}

#[test]
fn test_parse_list_comp_with_filter() {
    insta::assert_snapshot!(parse_expr("[x for x in list if x > 0]").unwrap().to_pretty_string(), @r"
    ListComp:
      Expr:
        Ident: x
      Var: x
      Iter:
        Ident: list
      Filter:
        BinOp: >
          Ident: x
          Int: 0
    ");
}

#[test]
fn test_parse_list_comp_complex() {
    // Function call in expression and filter
    insta::assert_snapshot!(parse_expr("[f(x) for x in items if pred(x)]").unwrap().to_pretty_string(), @r"
    ListComp:
      Expr:
        Call:
          Ident: f
          Args:
            Ident: x
      Var: x
      Iter:
        Ident: items
      Filter:
        Call:
          Ident: pred
          Args:
            Ident: x
    ");
}

#[test]
fn test_parse_list_comp_field_access() {
    // Field access in iterator expression
    insta::assert_snapshot!(parse_expr("[l for l in keys(lights)]").unwrap().to_pretty_string(), @r"
    ListComp:
      Expr:
        Ident: l
      Var: l
      Iter:
        Call:
          Ident: keys
          Args:
            Ident: lights
    ");
}

#[test]
fn test_parse_function_call() {
    insta::assert_snapshot!(parse_expr("foo()").unwrap().to_pretty_string(), @r"
    Call:
      Ident: foo
      Args: (none)
    ");
    insta::assert_snapshot!(parse_expr("add(1, 2)").unwrap().to_pretty_string(), @r"
    Call:
      Ident: add
      Args:
        Int: 1
        Int: 2
    ");
    insta::assert_snapshot!(parse_expr("f(a, b, c)").unwrap().to_pretty_string(), @r"
    Call:
      Ident: f
      Args:
        Ident: a
        Ident: b
        Ident: c
    ");
}

#[test]
fn test_parse_method_call() {
    insta::assert_snapshot!(parse_expr("list.filter(x)").unwrap().to_pretty_string(), @r"
    Call:
      Field: .filter
        Ident: list
      Args:
        Ident: x
    ");
    insta::assert_snapshot!(parse_expr("obj.method()").unwrap().to_pretty_string(), @r"
    Call:
      Field: .method
        Ident: obj
      Args: (none)
    ");
}

#[test]
fn test_parse_named_args() {
    // Single named argument
    insta::assert_snapshot!(parse_expr("func(x = 1)").unwrap().to_pretty_string(), @r"
    Call:
      Ident: func
      Args:
        Named: x
          Int: 1
    ");
    // Multiple named arguments
    insta::assert_snapshot!(parse_expr("func(x = 1, y = 2)").unwrap().to_pretty_string(), @r"
    Call:
      Ident: func
      Args:
        Named: x
          Int: 1
        Named: y
          Int: 2
    ");
    // Mixed positional and named
    insta::assert_snapshot!(parse_expr("func(a, x = 1)").unwrap().to_pretty_string(), @r"
    Call:
      Ident: func
      Args:
        Ident: a
        Named: x
          Int: 1
    ");
    // Named with complex expression
    insta::assert_snapshot!(parse_expr("wait(5min, retry = cancel)").unwrap().to_pretty_string(), @r"
    Call:
      Ident: wait
      Args:
        UnitLiteral: 5min
        Named: retry
          Ident: cancel
    ");
}

#[test]
fn test_parse_chained_calls() {
    insta::assert_snapshot!(parse_expr("a.b().c()").unwrap().to_pretty_string(), @r"
    Call:
      Field: .c
        Call:
          Field: .b
            Ident: a
          Args: (none)
      Args: (none)
    ");
    insta::assert_snapshot!(parse_expr("f(x).g(y)").unwrap().to_pretty_string(), @r"
    Call:
      Field: .g
        Call:
          Ident: f
          Args:
            Ident: x
      Args:
        Ident: y
    ");
}

#[test]
fn test_parse_if_expr() {
    insta::assert_snapshot!(parse_expr("if true { 1 } else { 2 }").unwrap().to_pretty_string(), @r"
    If:
      Cond:
        Bool: true
      Then:
        ExprStmt:
          Int: 1
      Else:
        ExprStmt:
          Int: 2
    ");
    insta::assert_snapshot!(parse_expr("if a { b } else { c }").unwrap().to_pretty_string(), @r"
    If:
      Cond:
        Ident: a
      Then:
        ExprStmt:
          Ident: b
      Else:
        ExprStmt:
          Ident: c
    ");
}

#[test]
fn test_parse_if_with_complex_condition() {
    insta::assert_snapshot!(parse_expr("if x > 5 && y < 10 { foo() } else { bar() }").unwrap().to_pretty_string(), @r"
    If:
      Cond:
        BinOp: &&
          BinOp: >
            Ident: x
            Int: 5
          BinOp: <
            Ident: y
            Int: 10
      Then:
        ExprStmt:
          Call:
            Ident: foo
            Args: (none)
      Else:
        ExprStmt:
          Call:
            Ident: bar
            Args: (none)
    ");
}

#[test]
fn test_parse_if_with_multiple_stmts() {
    insta::assert_snapshot!(parse_expr("if true { let x = 1; x } else { let y = 2; y }").unwrap().to_pretty_string(), @r"
    If:
      Cond:
        Bool: true
      Then:
        Let: x
          Int: 1
        ExprStmt:
          Ident: x
      Else:
        Let: y
          Int: 2
        ExprStmt:
          Ident: y
    ");
}

#[test]
fn test_parse_nested_if() {
    insta::assert_snapshot!(parse_expr("if a { if b { 1 } else { 2 } } else { 3 }").unwrap().to_pretty_string(), @r"
    If:
      Cond:
        Ident: a
      Then:
        ExprStmt:
          If:
            Cond:
              Ident: b
            Then:
              ExprStmt:
                Int: 1
            Else:
              ExprStmt:
                Int: 2
      Else:
        ExprStmt:
          Int: 3
    ");
}

#[test]
fn test_parse_automation() {
    insta::assert_snapshot!(crate::automations::parse("observer {} /true/ { let x = 42; }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
      Filter:
        Bool: true
      Body:
        Let: x
          Int: 42
    ");
    insta::assert_snapshot!(crate::automations::parse("mutator {} /a == b/ { foo() }").unwrap().to_pretty_string(), @r"
    Automation: mutator
      Pattern:
        PatternStruct:
      Filter:
        BinOp: ==
          Ident: a
          Ident: b
      Body:
        ExprStmt:
          Call:
            Ident: foo
            Args: (none)
    ");
}

#[test]
fn test_parse_automation_multiple_stmts() {
    insta::assert_snapshot!(crate::automations::parse("observer {} /true/ { let x = 1; let y = 2; x + y }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
      Filter:
        Bool: true
      Body:
        Let: x
          Int: 1
        Let: y
          Int: 2
        ExprStmt:
          BinOp: +
            Ident: x
            Ident: y
    ");
}

#[test]
fn test_parse_automation_no_filter() {
    // Filter is optional
    insta::assert_snapshot!(crate::automations::parse("observer {} { x }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
      Body:
        ExprStmt:
          Ident: x
    ");
}

#[test]
fn test_parse_complex_expr() {
    // Operator precedence
    insta::assert_snapshot!(parse_expr("a + b * c - d / e").unwrap().to_pretty_string(), @r"
    BinOp: -
      BinOp: +
        Ident: a
        BinOp: *
          Ident: b
          Ident: c
      BinOp: /
        Ident: d
        Ident: e
    ");
    // Mixed operators
    insta::assert_snapshot!(parse_expr("a == b && c != d || e < f").unwrap().to_pretty_string(), @r"
    BinOp: ||
      BinOp: &&
        BinOp: ==
          Ident: a
          Ident: b
        BinOp: !=
          Ident: c
          Ident: d
      BinOp: <
        Ident: e
        Ident: f
    ");
}

#[test]
fn test_parse_pattern_empty() {
    insta::assert_snapshot!(crate::automations::parse("observer {} /true/ { x }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
      Filter:
        Bool: true
      Body:
        ExprStmt:
          Ident: x
    ");
}

#[test]
fn test_parse_pattern_single_field() {
    insta::assert_snapshot!(crate::automations::parse("observer { x } /true/ { x }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: x
      Filter:
        Bool: true
      Body:
        ExprStmt:
          Ident: x
    ");
}

#[test]
fn test_parse_pattern_multiple_fields() {
    insta::assert_snapshot!(crate::automations::parse("observer { x, y, z } /true/ { x }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: x
          FieldPattern: y
          FieldPattern: z
      Filter:
        Bool: true
      Body:
        ExprStmt:
          Ident: x
    ");
}

#[test]
fn test_parse_pattern_with_rest() {
    insta::assert_snapshot!(crate::automations::parse("observer { x, y, ... } /true/ { x }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: x
          FieldPattern: y
          Rest: ...
      Filter:
        Bool: true
      Body:
        ExprStmt:
          Ident: x
    ");
}

#[test]
fn test_parse_pattern_trailing_comma() {
    insta::assert_snapshot!(crate::automations::parse("observer { x, y, } /true/ { x }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: x
          FieldPattern: y
      Filter:
        Bool: true
      Body:
        ExprStmt:
          Ident: x
    ");
}

#[test]
fn test_parse_pattern_nested() {
    insta::assert_snapshot!(crate::automations::parse("observer { x = { inner } } /true/ { x }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: x
            PatternStruct:
              FieldPattern: inner
      Filter:
        Bool: true
      Body:
        ExprStmt:
          Ident: x
    ");
}

#[test]
fn test_parse_pattern_nested_with_rest() {
    insta::assert_snapshot!(crate::automations::parse("observer { x = { a, b, ... }, y } /true/ { x }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: x
            PatternStruct:
              FieldPattern: a
              FieldPattern: b
              Rest: ...
          FieldPattern: y
      Filter:
        Bool: true
      Body:
        ExprStmt:
          Ident: x
    ");
}

#[test]
fn test_parse_struct_lit_empty() {
    insta::assert_snapshot!(parse_expr("Name {}").unwrap().to_pretty_string(), @r"
    StructLit: Name
    ");
}

#[test]
fn test_parse_struct_lit_single_field() {
    insta::assert_snapshot!(parse_expr("Point { x: 1 }").unwrap().to_pretty_string(), @r"
    StructLit: Point
      Field: x
        Int: 1
    ");
}

#[test]
fn test_parse_struct_lit_multiple_fields() {
    insta::assert_snapshot!(parse_expr("Point { x: 1; y: 2 }").unwrap().to_pretty_string(), @r"
    StructLit: Point
      Field: x
        Int: 1
      Field: y
        Int: 2
    ");
}

#[test]
fn test_parse_struct_lit_inherit() {
    insta::assert_snapshot!(parse_expr("Point { inherit x }").unwrap().to_pretty_string(), @r"
    StructLit: Point
      Inherit: x
    ");
}

#[test]
fn test_parse_struct_lit_spread() {
    insta::assert_snapshot!(parse_expr("Point { ...other }").unwrap().to_pretty_string(), @r"
    StructLit: Point
      Spread: other
    ");
}

#[test]
fn test_parse_struct_lit_mixed() {
    insta::assert_snapshot!(parse_expr("Config { x: 1; inherit y; ...defaults }").unwrap().to_pretty_string(), @r"
    StructLit: Config
      Field: x
        Int: 1
      Inherit: y
      Spread: defaults
    ");
}

#[test]
fn test_parse_struct_lit_nested() {
    insta::assert_snapshot!(parse_expr("Outer { inner: Inner { x: 1 } }").unwrap().to_pretty_string(), @r"
    StructLit: Outer
      Field: inner
        StructLit: Inner
          Field: x
            Int: 1
    ");
}

#[test]
fn test_parse_path_basic() {
    insta::assert_snapshot!(parse_expr("Event::ZoneChange").unwrap().to_pretty_string(), @r"
    Path:
      Segment: Event
      Segment: ZoneChange
    ");
}

#[test]
fn test_parse_path_three_segments() {
    insta::assert_snapshot!(parse_expr("Foo::Bar::Baz").unwrap().to_pretty_string(), @r"
    Path:
      Segment: Foo
      Segment: Bar
      Segment: Baz
    ");
}

#[test]
fn test_parse_path_call() {
    // Path used as function call
    insta::assert_snapshot!(parse_expr("Event::LightOff(l)").unwrap().to_pretty_string(), @r"
    Call:
      Path:
        Segment: Event
        Segment: LightOff
      Args:
        Ident: l
    ");
}

#[test]
fn test_parse_path_in_comparison() {
    insta::assert_snapshot!(parse_expr("event.type == Event::ZoneChange").unwrap().to_pretty_string(), @r"
    BinOp: ==
      Field: .type
        Ident: event
      Path:
        Segment: Event
        Segment: ZoneChange
    ");
}

#[test]
fn test_parse_design_doc_mutator() {
    let src = r#"
mutator {
    event,
    state = { sun = { azimuth, ... }, ... },
    ...
} /event.type == Event::LightOn/ {
    let brightness = azimuth * 0.5;
    let colour = azimuth * 1.2;

    Event {
        inherit brightness colour;
        ...event
    }
}
"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Automation: mutator
      Pattern:
        PatternStruct:
          FieldPattern: event
          FieldPattern: state
            PatternStruct:
              FieldPattern: sun
                PatternStruct:
                  FieldPattern: azimuth
                  Rest: ...
              Rest: ...
          Rest: ...
      Filter:
        BinOp: ==
          Field: .type
            Ident: event
          Path:
            Segment: Event
            Segment: LightOn
      Body:
        Let: brightness
          BinOp: *
            Ident: azimuth
            Float: 0.5
        Let: colour
          BinOp: *
            Ident: azimuth
            Float: 1.2
        ExprStmt:
          StructLit: Event
            Inherit: brightness
            Inherit: colour
            Spread: event
    ");
}

#[test]
fn test_parse_design_doc_observer() {
    let src = r#"
observer {
    event,
    state = {
        lights,
        person_tracker,
        zone,
        helpers = { guest_mode, ... },
        ...
    },
    ...
} /!guest_mode
   && event.type == Event::ZoneChange
   && event.device == person_tracker.jake
   && event.from == zone.home/ {

    if await sleep_unique(5min) {
        [ Event::LightOff(l) for l in keys(lights) ]
    } else {
        []
    }
}
"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: event
          FieldPattern: state
            PatternStruct:
              FieldPattern: lights
              FieldPattern: person_tracker
              FieldPattern: zone
              FieldPattern: helpers
                PatternStruct:
                  FieldPattern: guest_mode
                  Rest: ...
              Rest: ...
          Rest: ...
      Filter:
        BinOp: &&
          BinOp: &&
            BinOp: &&
              UnaryOp: !
                Ident: guest_mode
              BinOp: ==
                Field: .type
                  Ident: event
                Path:
                  Segment: Event
                  Segment: ZoneChange
            BinOp: ==
              Field: .device
                Ident: event
              Field: .jake
                Ident: person_tracker
          BinOp: ==
            Field: .from
              Ident: event
            Field: .home
              Ident: zone
      Body:
        ExprStmt:
          If:
            Cond:
              UnaryOp: await
                Call:
                  Ident: sleep_unique
                  Args:
                    UnitLiteral: 5min
            Then:
              ExprStmt:
                ListComp:
                  Expr:
                    Call:
                      Path:
                        Segment: Event
                        Segment: LightOff
                      Args:
                        Ident: l
                  Var: l
                  Iter:
                    Call:
                      Ident: keys
                      Args:
                        Ident: lights
            Else:
              ExprStmt:
                List: (empty)
    ");
}

#[test]
fn test_parse_return_basic() {
    insta::assert_snapshot!(crate::automations::parse("observer {} /true/ { return x; }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
      Filter:
        Bool: true
      Body:
        Return:
          Ident: x
    ");
}

#[test]
fn test_parse_return_with_expression() {
    insta::assert_snapshot!(crate::automations::parse("observer {} /true/ { return Event::LightOff(l); }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
      Filter:
        Bool: true
      Body:
        Return:
          Call:
            Path:
              Segment: Event
              Segment: LightOff
            Args:
              Ident: l
    ");
}

#[test]
fn test_parse_return_empty_list() {
    insta::assert_snapshot!(crate::automations::parse("observer {} /true/ { return []; }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
      Filter:
        Bool: true
      Body:
        Return:
          List: (empty)
    ");
}

#[test]
fn test_parse_if_without_else() {
    // One-sided if statement (no else branch)
    insta::assert_snapshot!(crate::automations::parse("observer {} /true/ { if x { return []; } }").unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
      Filter:
        Bool: true
      Body:
        ExprStmt:
          If:
            Cond:
              Ident: x
            Then:
              Return:
                List: (empty)
    ");
}

#[test]
fn test_parse_early_exit_pattern() {
    // Early exit pattern: if guard returns early, then implicit return of expression
    let src = r#"
observer {
    event,
    state = { lights, person_tracker, ... },
    ...
} /event.type == Event::Motion/ {
    if !person_tracker.anyone_home {
        return [];
    }

    [ Event::LightOn(l) for l in keys(lights) ]
}
"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Automation: observer
      Pattern:
        PatternStruct:
          FieldPattern: event
          FieldPattern: state
            PatternStruct:
              FieldPattern: lights
              FieldPattern: person_tracker
              Rest: ...
          Rest: ...
      Filter:
        BinOp: ==
          Field: .type
            Ident: event
          Path:
            Segment: Event
            Segment: Motion
      Body:
        ExprStmt:
          If:
            Cond:
              UnaryOp: !
                Field: .anyone_home
                  Ident: person_tracker
            Then:
              Return:
                List: (empty)
        ExprStmt:
          ListComp:
            Expr:
              Call:
                Path:
                  Segment: Event
                  Segment: LightOn
                Args:
                  Ident: l
            Var: l
            Iter:
              Call:
                Ident: keys
                Args:
                  Ident: lights
    ");
}

#[test]
fn test_parse_template_single_param() {
    let src = r#"{ lights: Light }: [ observer {} { x } ]"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Template:
      Params:
        Param: lights
          Type::Named: Light
      Automations:
        Automation: observer
          Pattern:
            PatternStruct:
          Body:
            ExprStmt:
              Ident: x
    ");
}

#[test]
fn test_parse_template_multiple_params() {
    let src = r#"{ lights: Set<Light>, timeout: Duration }: [ observer {} { x } ]"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Template:
      Params:
        Param: lights
          Type::Set:
            Type::Named: Light
        Param: timeout
          Type::Named: Duration
      Automations:
        Automation: observer
          Pattern:
            PatternStruct:
          Body:
            ExprStmt:
              Ident: x
    ");
}

#[test]
fn test_parse_template_complex_types() {
    let src = r#"{ items: [Event], mapping: Map<String, Light>, maybe: Option<Zone> }: [ observer {} { x } ]"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Template:
      Params:
        Param: items
          Type::List:
            Type::Named: Event
        Param: mapping
          Type::Map:
            Key:
              Type::Named: String
            Value:
              Type::Named: Light
        Param: maybe
          Type::Option:
            Type::Named: Zone
      Automations:
        Automation: observer
          Pattern:
            PatternStruct:
          Body:
            ExprStmt:
              Ident: x
    ");
}

#[test]
fn test_parse_template_nested_types() {
    let src = r#"{ nested: Set<Option<Light>> }: [ observer {} { x } ]"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Template:
      Params:
        Param: nested
          Type::Set:
            Type::Option:
              Type::Named: Light
      Automations:
        Automation: observer
          Pattern:
            PatternStruct:
          Body:
            ExprStmt:
              Ident: x
    ");
}

#[test]
fn test_parse_template_multiple_automations() {
    let src = r#"
{ lights: Set<Light> }:
[
    observer {} { [] },
    mutator {} { x }
]
"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Template:
      Params:
        Param: lights
          Type::Set:
            Type::Named: Light
      Automations:
        Automation: observer
          Pattern:
            PatternStruct:
          Body:
            ExprStmt:
              List: (empty)
        Automation: mutator
          Pattern:
            PatternStruct:
          Body:
            ExprStmt:
              Ident: x
    ");
}

#[test]
fn test_parse_template_with_filter() {
    let src = r#"{ sensor: Sensor }: [ observer {} /event.type == Event::Motion/ { [] } ]"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Template:
      Params:
        Param: sensor
          Type::Named: Sensor
      Automations:
        Automation: observer
          Pattern:
            PatternStruct:
          Filter:
            BinOp: ==
              Field: .type
                Ident: event
              Path:
                Segment: Event
                Segment: Motion
          Body:
            ExprStmt:
              List: (empty)
    ");
}

#[test]
fn test_parse_template_design_doc_example() {
    // Example from docs/design/automations-language.md
    let src = r#"
{ target_lights: Set<Light> }:

[
  mutator {
    event,
    state = { sun = { azimuth, ... }, ... },
    ...
  } /event.type == Event::LightOn && event.device in target_lights/ {
    let brightness = azimuth * 0.5;
    let colour = azimuth * 1.2;

    Event {
      inherit brightness colour;
      ...event
    }
  },

  observer {
    event,
    state = { ... },
    ...
  } /event.type == Event::Tick/ {
    // Update lights on time tick
    [ Event::LightUpdate(l) for l in target_lights ]
  }
]
"#;
    insta::assert_snapshot!(crate::automations::parse(src).unwrap().to_pretty_string(), @r"
    Template:
      Params:
        Param: target_lights
          Type::Set:
            Type::Named: Light
      Automations:
        Automation: mutator
          Pattern:
            PatternStruct:
              FieldPattern: event
              FieldPattern: state
                PatternStruct:
                  FieldPattern: sun
                    PatternStruct:
                      FieldPattern: azimuth
                      Rest: ...
                  Rest: ...
              Rest: ...
          Filter:
            BinOp: &&
              BinOp: ==
                Field: .type
                  Ident: event
                Path:
                  Segment: Event
                  Segment: LightOn
              BinOp: in
                Field: .device
                  Ident: event
                Ident: target_lights
          Body:
            Let: brightness
              BinOp: *
                Ident: azimuth
                Float: 0.5
            Let: colour
              BinOp: *
                Ident: azimuth
                Float: 1.2
            ExprStmt:
              StructLit: Event
                Inherit: brightness
                Inherit: colour
                Spread: event
        Automation: observer
          Pattern:
            PatternStruct:
              FieldPattern: event
              FieldPattern: state
                PatternStruct:
                  Rest: ...
              Rest: ...
          Filter:
            BinOp: ==
              Field: .type
                Ident: event
              Path:
                Segment: Event
                Segment: Tick
          Body:
            ExprStmt:
              ListComp:
                Expr:
                  Call:
                    Path:
                      Segment: Event
                      Segment: LightUpdate
                    Args:
                      Ident: l
                Var: l
                Iter:
                  Ident: target_lights
    ");
}
