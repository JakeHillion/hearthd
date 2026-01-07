use chumsky::prelude::*;

use super::desugar;
use crate::automations::lexer::Token;
use crate::automations::repr::ast::Expr;
use crate::automations::repr::ast::Spanned;
use crate::automations::repr::pretty_print::PrettyPrint;

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
    let result = crate::automations::parser::expr_parser()
        .parse(
            tokens
                .as_slice()
                .map((input_len..input_len).into(), |(t, s)| (t, s)),
        )
        .into_result()
        .map_err(|errs| errs.into_iter().map(|e| e.into_owned()).collect());
    result
}

/// Returns (parsed AST pretty string, lowered AST pretty string)
fn parse_and_desugar(input: &str) -> (String, String) {
    let ast = parse_expr(input).expect("parsing should succeed");
    let ast_pretty = ast.to_pretty_string();
    let lowered = desugar(ast);
    (ast_pretty, lowered.to_pretty_string())
}

// =============================================================================
// List Comprehension Tests
// =============================================================================

#[test]
fn test_desugar_simple_list_comp() {
    let (ast, lowered) = parse_and_desugar("[x for x in list]");
    insta::assert_snapshot!(ast, @"
    ListComp:
      Expr:
        Ident: x
      Var: x
      Iter:
        Ident: list
    ");
    insta::assert_snapshot!(lowered, @"
    Origin: ListComp @ 0..17
    Block:
      Stmts:
        Origin: ListComp @ 0..17
        LetMut: __result0
          Origin: ListComp @ 0..17
          MutableList
        Origin: ListComp @ 0..17
        For:
          Var: x
          Iter:
            Origin: Direct @ 12..16
            Ident: list
          Body:
            Origin: ListComp @ 0..17
            Push: __result0
              Origin: Direct @ 1..2
              Ident: x
      Result:
        Origin: ListComp @ 0..17
        Ident: __result0
    ");
}

#[test]
fn test_desugar_list_comp_with_expr() {
    let (ast, lowered) = parse_and_desugar("[x * 2 for x in items]");
    insta::assert_snapshot!(ast, @"
    ListComp:
      Expr:
        BinOp: *
          Ident: x
          Int: 2
      Var: x
      Iter:
        Ident: items
    ");
    insta::assert_snapshot!(lowered, @"
    Origin: ListComp @ 0..22
    Block:
      Stmts:
        Origin: ListComp @ 0..22
        LetMut: __result0
          Origin: ListComp @ 0..22
          MutableList
        Origin: ListComp @ 0..22
        For:
          Var: x
          Iter:
            Origin: Direct @ 16..21
            Ident: items
          Body:
            Origin: ListComp @ 0..22
            Push: __result0
              Origin: Direct @ 1..6
              BinOp: *
                Origin: Direct @ 1..2
                Ident: x
                Origin: Direct @ 5..6
                Int: 2
      Result:
        Origin: ListComp @ 0..22
        Ident: __result0
    ");
}

#[test]
fn test_desugar_list_comp_with_filter() {
    let (ast, lowered) = parse_and_desugar("[x for x in list if x > 0]");
    insta::assert_snapshot!(ast, @"
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
    insta::assert_snapshot!(lowered, @"
    Origin: ListComp @ 0..26
    Block:
      Stmts:
        Origin: ListComp @ 0..26
        LetMut: __result0
          Origin: ListComp @ 0..26
          MutableList
        Origin: ListComp @ 0..26
        For:
          Var: x
          Iter:
            Origin: Direct @ 12..16
            Ident: list
          Body:
            Origin: ListComp @ 0..26
            ExprStmt:
              Origin: ListComp @ 0..26
              If:
                Cond:
                  Origin: Direct @ 20..25
                  BinOp: >
                    Origin: Direct @ 20..21
                    Ident: x
                    Origin: Direct @ 24..25
                    Int: 0
                Then:
                  Origin: ListComp @ 0..26
                  Push: __result0
                    Origin: Direct @ 1..2
                    Ident: x
      Result:
        Origin: ListComp @ 0..26
        Ident: __result0
    ");
}

#[test]
fn test_desugar_list_comp_complex() {
    let (ast, lowered) = parse_and_desugar("[f(x) for x in items if pred(x)]");
    insta::assert_snapshot!(ast, @"
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
    insta::assert_snapshot!(lowered, @"
    Origin: ListComp @ 0..32
    Block:
      Stmts:
        Origin: ListComp @ 0..32
        LetMut: __result0
          Origin: ListComp @ 0..32
          MutableList
        Origin: ListComp @ 0..32
        For:
          Var: x
          Iter:
            Origin: Direct @ 15..20
            Ident: items
          Body:
            Origin: ListComp @ 0..32
            ExprStmt:
              Origin: ListComp @ 0..32
              If:
                Cond:
                  Origin: Direct @ 24..31
                  Call:
                    Origin: Direct @ 24..28
                    Ident: pred
                    Args:
                      Origin: Direct @ 29..30
                      Origin: Direct @ 29..30
                      Ident: x
                Then:
                  Origin: ListComp @ 0..32
                  Push: __result0
                    Origin: Direct @ 1..5
                    Call:
                      Origin: Direct @ 1..2
                      Ident: f
                      Args:
                        Origin: Direct @ 3..4
                        Origin: Direct @ 3..4
                        Ident: x
      Result:
        Origin: ListComp @ 0..32
        Ident: __result0
    ");
}

#[test]
fn test_desugar_list_comp_with_path() {
    let (ast, lowered) = parse_and_desugar("[Event::LightOff(l) for l in keys(lights)]");
    insta::assert_snapshot!(ast, @"
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
    ");
    insta::assert_snapshot!(lowered, @"
    Origin: ListComp @ 0..42
    Block:
      Stmts:
        Origin: ListComp @ 0..42
        LetMut: __result0
          Origin: ListComp @ 0..42
          MutableList
        Origin: ListComp @ 0..42
        For:
          Var: l
          Iter:
            Origin: Direct @ 29..41
            Call:
              Origin: Direct @ 29..33
              Ident: keys
              Args:
                Origin: Direct @ 34..40
                Origin: Direct @ 34..40
                Ident: lights
          Body:
            Origin: ListComp @ 0..42
            Push: __result0
              Origin: Direct @ 1..19
              Call:
                Origin: Direct @ 1..16
                Path:
                  Segment: Event
                  Segment: LightOff
                Args:
                  Origin: Direct @ 17..18
                  Origin: Direct @ 17..18
                  Ident: l
      Result:
        Origin: ListComp @ 0..42
        Ident: __result0
    ");
}

// =============================================================================
// Pass-through Tests (non-ListComp expressions)
// =============================================================================

#[test]
fn test_desugar_passthrough_int() {
    let (ast, lowered) = parse_and_desugar("42");
    insta::assert_snapshot!(ast, @"Int: 42");
    insta::assert_snapshot!(lowered, @"
    Origin: Direct @ 0..2
    Int: 42
    ");
}

#[test]
fn test_desugar_passthrough_binop() {
    let (ast, lowered) = parse_and_desugar("1 + 2");
    insta::assert_snapshot!(ast, @"
    BinOp: +
      Int: 1
      Int: 2
    ");
    insta::assert_snapshot!(lowered, @"
    Origin: Direct @ 0..5
    BinOp: +
      Origin: Direct @ 0..1
      Int: 1
      Origin: Direct @ 4..5
      Int: 2
    ");
}

#[test]
fn test_desugar_passthrough_list() {
    let (ast, lowered) = parse_and_desugar("[1, 2, 3]");
    insta::assert_snapshot!(ast, @"
    List:
      Int: 1
      Int: 2
      Int: 3
    ");
    insta::assert_snapshot!(lowered, @"
    Origin: Direct @ 0..9
    List:
      Origin: Direct @ 1..2
      Int: 1
      Origin: Direct @ 4..5
      Int: 2
      Origin: Direct @ 7..8
      Int: 3
    ");
}

#[test]
fn test_desugar_passthrough_if() {
    let (ast, lowered) = parse_and_desugar("if cond { x }");
    insta::assert_snapshot!(ast, @"
    If:
      Cond:
        Ident: cond
      Then:
        ExprStmt:
          Ident: x
    ");
    insta::assert_snapshot!(lowered, @"
    Origin: Direct @ 0..13
    If:
      Cond:
        Origin: Direct @ 3..7
        Ident: cond
      Then:
        Origin: Direct @ 10..11
        ExprStmt:
          Origin: Direct @ 10..11
          Ident: x
    ");
}

// =============================================================================
// Nested Tests
// =============================================================================

#[test]
fn test_desugar_nested_list_comp_in_if() {
    let (ast, lowered) = parse_and_desugar("if cond { [x for x in items] } else { [] }");
    insta::assert_snapshot!(ast, @"
    If:
      Cond:
        Ident: cond
      Then:
        ExprStmt:
          ListComp:
            Expr:
              Ident: x
            Var: x
            Iter:
              Ident: items
      Else:
        ExprStmt:
          List: (empty)
    ");
    insta::assert_snapshot!(lowered, @"
    Origin: Direct @ 0..42
    If:
      Cond:
        Origin: Direct @ 3..7
        Ident: cond
      Then:
        Origin: ListComp @ 10..28
        ExprStmt:
          Origin: ListComp @ 10..28
          Block:
            Stmts:
              Origin: ListComp @ 10..28
              LetMut: __result0
                Origin: ListComp @ 10..28
                MutableList
              Origin: ListComp @ 10..28
              For:
                Var: x
                Iter:
                  Origin: Direct @ 22..27
                  Ident: items
                Body:
                  Origin: ListComp @ 10..28
                  Push: __result0
                    Origin: Direct @ 11..12
                    Ident: x
            Result:
              Origin: ListComp @ 10..28
              Ident: __result0
      Else:
        Origin: Direct @ 38..40
        ExprStmt:
          Origin: Direct @ 38..40
          List: (empty)
    ");
}

#[test]
fn test_desugar_nested_list_comp() {
    let (ast, lowered) = parse_and_desugar("[[x for x in row] for row in matrix]");
    insta::assert_snapshot!(ast, @"
    ListComp:
      Expr:
        ListComp:
          Expr:
            Ident: x
          Var: x
          Iter:
            Ident: row
      Var: row
      Iter:
        Ident: matrix
    ");
    insta::assert_snapshot!(lowered, @"
    Origin: ListComp @ 0..36
    Block:
      Stmts:
        Origin: ListComp @ 0..36
        LetMut: __result0
          Origin: ListComp @ 0..36
          MutableList
        Origin: ListComp @ 0..36
        For:
          Var: row
          Iter:
            Origin: Direct @ 29..35
            Ident: matrix
          Body:
            Origin: ListComp @ 0..36
            Push: __result0
              Origin: ListComp @ 1..17
              Block:
                Stmts:
                  Origin: ListComp @ 1..17
                  LetMut: __result1
                    Origin: ListComp @ 1..17
                    MutableList
                  Origin: ListComp @ 1..17
                  For:
                    Var: x
                    Iter:
                      Origin: Direct @ 13..16
                      Ident: row
                    Body:
                      Origin: ListComp @ 1..17
                      Push: __result1
                        Origin: Direct @ 2..3
                        Ident: x
                Result:
                  Origin: ListComp @ 1..17
                  Ident: __result1
      Result:
        Origin: ListComp @ 0..36
        Ident: __result0
    ");
}

#[test]
fn test_desugar_list_comp_with_field_access() {
    let (ast, lowered) = parse_and_desugar("[item.value for item in list]");
    insta::assert_snapshot!(ast, @"
    ListComp:
      Expr:
        Field: .value
          Ident: item
      Var: item
      Iter:
        Ident: list
    ");
    insta::assert_snapshot!(lowered, @"
    Origin: ListComp @ 0..29
    Block:
      Stmts:
        Origin: ListComp @ 0..29
        LetMut: __result0
          Origin: ListComp @ 0..29
          MutableList
        Origin: ListComp @ 0..29
        For:
          Var: item
          Iter:
            Origin: Direct @ 24..28
            Ident: list
          Body:
            Origin: ListComp @ 0..29
            Push: __result0
              Origin: Direct @ 1..11
              Field: .value
                Origin: Direct @ 1..5
                Ident: item
      Result:
        Origin: ListComp @ 0..29
        Ident: __result0
    ");
}
