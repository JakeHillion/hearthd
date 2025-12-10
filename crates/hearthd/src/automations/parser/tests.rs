use chumsky::prelude::*;

use super::expr_parser;
use crate::automations::ast::*;
use crate::automations::lexer::Token;

fn parse_expr(input: &str) -> Result<Spanned<Expr>, Vec<Rich<'static, Token>>> {
    let tokens = crate::automations::lexer::lexer()
        .parse(input)
        .into_result()
        .map_err(|errs| {
            // Convert lexer errors (Rich<char>) to parser errors (Rich<Token>)
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
    assert_eq!(parse_expr("42").unwrap().node, Expr::Int(42));
    assert_eq!(
        parse_expr("3.14").unwrap().node,
        Expr::Float("3.14".to_string())
    );
    assert_eq!(
        parse_expr("\"hello\"").unwrap().node,
        Expr::String("hello".to_string())
    );
    assert_eq!(parse_expr("true").unwrap().node, Expr::Bool(true));
    assert_eq!(parse_expr("false").unwrap().node, Expr::Bool(false));
}

#[test]
fn test_parse_unit_literals() {
    assert_eq!(
        parse_expr("5min").unwrap().node,
        Expr::UnitLiteral {
            value: "5".to_string(),
            unit: UnitType::Minutes
        }
    );
    assert_eq!(
        parse_expr("90deg").unwrap().node,
        Expr::UnitLiteral {
            value: "90".to_string(),
            unit: UnitType::Degrees
        }
    );
}

#[test]
fn test_parse_identifiers() {
    assert_eq!(
        parse_expr("foo").unwrap().node,
        Expr::Ident("foo".to_string())
    );
}

#[test]
fn test_parse_binary_ops() {
    // Test simple addition
    let result = parse_expr("1 + 2").unwrap();
    match result.node {
        Expr::BinOp { op, left, right } => {
            assert_eq!(op, BinOp::Add);
            assert_eq!(left.node, Expr::Int(1));
            assert_eq!(right.node, Expr::Int(2));
        }
        _ => panic!("Expected binary op"),
    }

    // Test precedence: multiplication before addition
    let result = parse_expr("1 + 2 * 3").unwrap();
    match result.node {
        Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } => {
            assert_eq!(left.node, Expr::Int(1));
            match right.node {
                Expr::BinOp {
                    op: BinOp::Mul,
                    left: mul_left,
                    right: mul_right,
                } => {
                    assert_eq!(mul_left.node, Expr::Int(2));
                    assert_eq!(mul_right.node, Expr::Int(3));
                }
                _ => panic!("Expected multiplication"),
            }
        }
        _ => panic!("Expected addition at top level"),
    }
}

#[test]
fn test_parse_comparison() {
    let result = parse_expr("x == 5").unwrap();
    match result.node {
        Expr::BinOp { op, left, right } => {
            assert_eq!(op, BinOp::Eq);
            assert_eq!(left.node, Expr::Ident("x".to_string()));
            assert_eq!(right.node, Expr::Int(5));
        }
        _ => panic!("Expected binary op"),
    }
}

#[test]
fn test_parse_logical_and() {
    let result = parse_expr("true && false").unwrap();
    match result.node {
        Expr::BinOp { op, left, right } => {
            assert_eq!(op, BinOp::And);
            assert_eq!(left.node, Expr::Bool(true));
            assert_eq!(right.node, Expr::Bool(false));
        }
        _ => panic!("Expected binary op"),
    }
}

#[test]
fn test_parse_unary_ops() {
    let result = parse_expr("!true").unwrap();
    match result.node {
        Expr::UnaryOp { op, expr } => {
            assert_eq!(op, UnaryOp::Not);
            assert_eq!(expr.node, Expr::Bool(true));
        }
        _ => panic!("Expected unary op"),
    }

    let result = parse_expr("-42").unwrap();
    match result.node {
        Expr::UnaryOp { op, expr } => {
            assert_eq!(op, UnaryOp::Neg);
            assert_eq!(expr.node, Expr::Int(42));
        }
        _ => panic!("Expected unary op"),
    }
}

#[test]
fn test_parse_field_access() {
    let result = parse_expr("event.type").unwrap();
    match result.node {
        Expr::Field { expr, field } => {
            assert_eq!(expr.node, Expr::Ident("event".to_string()));
            assert_eq!(field, "type");
        }
        _ => panic!("Expected field access"),
    }
}

#[test]
fn test_parse_optional_chaining() {
    let result = parse_expr("person?.location").unwrap();
    match result.node {
        Expr::OptionalField { expr, field } => {
            assert_eq!(expr.node, Expr::Ident("person".to_string()));
            assert_eq!(field, "location");
        }
        _ => panic!("Expected optional chaining"),
    }
}

#[test]
fn test_parse_list() {
    let result = parse_expr("[1, 2, 3]").unwrap();
    match result.node {
        Expr::List(items) => {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0].node, Expr::Int(1));
            assert_eq!(items[1].node, Expr::Int(2));
            assert_eq!(items[2].node, Expr::Int(3));
        }
        _ => panic!("Expected list"),
    }
}
