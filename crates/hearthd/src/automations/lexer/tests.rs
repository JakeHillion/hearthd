use chumsky::Parser;

use super::Token;
use super::lexer;
use crate::automations::repr::ast::UnitType;

#[test]
fn test_lex_integers() {
    let input = "42 0 12345";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::Int(42), (0..2).into()),
            (Token::Int(0), (3..4).into()),
            (Token::Int(12345), (5..10).into()),
        ]
    );
}

#[test]
fn test_lex_floats() {
    let input = "3.14 0.5 123.456";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::Float("3.14".to_string()), (0..4).into()),
            (Token::Float("0.5".to_string()), (5..8).into()),
            (Token::Float("123.456".to_string()), (9..16).into()),
        ]
    );
}

#[test]
fn test_lex_strings() {
    let input = r#""hello" "world" "with \"escapes\"" "#;
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::String("hello".to_string()), (0..7).into()),
            (Token::String("world".to_string()), (8..15).into()),
            // Note: span is 16..34 because the input has escape sequences that are shorter after parsing
            (
                Token::String("with \"escapes\"".to_string()),
                (16..34).into()
            ),
        ]
    );
}

#[test]
fn test_lex_bools() {
    let input = "true false";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::Bool(true), (0..4).into()),
            (Token::Bool(false), (5..10).into()),
        ]
    );
}

#[test]
fn test_lex_keywords() {
    let input = "observer mutator let if else for in await inherit";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::Observer, (0..8).into()),
            (Token::Mutator, (9..16).into()),
            (Token::Let, (17..20).into()),
            (Token::If, (21..23).into()),
            (Token::Else, (24..28).into()),
            (Token::For, (29..32).into()),
            (Token::In, (33..35).into()),
            (Token::Await, (36..41).into()),
            (Token::Inherit, (42..49).into()),
        ]
    );
}

#[test]
fn test_lex_identifiers() {
    let input = "foo bar_baz myVar123";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::Ident("foo".to_string()), (0..3).into()),
            (Token::Ident("bar_baz".to_string()), (4..11).into()),
            (Token::Ident("myVar123".to_string()), (12..20).into()),
        ]
    );
}

#[test]
fn test_lex_operators() {
    let input = "+ - * / % == != < <= > >= && || !";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::Plus, (0..1).into()),
            (Token::Minus, (2..3).into()),
            (Token::Star, (4..5).into()),
            (Token::Slash, (6..7).into()),
            (Token::Percent, (8..9).into()),
            (Token::Eq, (10..12).into()),
            (Token::Ne, (13..15).into()),
            (Token::Lt, (16..17).into()),
            (Token::Le, (18..20).into()),
            (Token::Gt, (21..22).into()),
            (Token::Ge, (23..25).into()),
            (Token::And, (26..28).into()),
            (Token::Or, (29..31).into()),
            (Token::Not, (32..33).into()),
        ]
    );
}

#[test]
fn test_lex_unit_literals() {
    let input = "5min 2.5h 90deg 20c";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (
                Token::UnitLiteral {
                    value: "5".to_string(),
                    unit: UnitType::Minutes
                },
                (0..4).into()
            ),
            (
                Token::UnitLiteral {
                    value: "2.5".to_string(),
                    unit: UnitType::Hours
                },
                (5..9).into()
            ),
            (
                Token::UnitLiteral {
                    value: "90".to_string(),
                    unit: UnitType::Degrees
                },
                (10..15).into()
            ),
            (
                Token::UnitLiteral {
                    value: "20".to_string(),
                    unit: UnitType::Celsius
                },
                (16..19).into()
            ),
        ]
    );
}

#[test]
fn test_lex_line_comments() {
    let input = "foo // this is a comment\nbar";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::Ident("foo".to_string()), (0..3).into()),
            (Token::Ident("bar".to_string()), (25..28).into()),
        ]
    );
}

#[test]
fn test_lex_block_comments() {
    let input = "foo /* block comment */ bar";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::Ident("foo".to_string()), (0..3).into()),
            (Token::Ident("bar".to_string()), (24..27).into()),
        ]
    );
}

#[test]
fn test_lex_multiline_block_comment() {
    let input = "foo /*\n  multi\n  line\n*/ bar";
    let result = lexer().parse(input).into_result().unwrap();
    assert_eq!(
        result,
        vec![
            (Token::Ident("foo".to_string()), (0..3).into()),
            (Token::Ident("bar".to_string()), (25..28).into()),
        ]
    );
}
