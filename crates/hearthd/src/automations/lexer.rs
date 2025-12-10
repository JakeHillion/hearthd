//! Lexer and token definitions for the HearthD Automations language.

use chumsky::input::MapExtra;
use chumsky::prelude::*;

use super::ast::Span;
use super::ast::UnitType;

/// A token in the automations language.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Token {
    // Literals
    Int(i64),
    String(String),
    Bool(bool),

    // Float and unit literals stored separately (floats don't impl Eq/Hash)
    Float(String), // Store as string for now
    UnitLiteral { value: String, unit: UnitType },

    // Identifiers and keywords
    Ident(String),

    // Keywords
    Observer,
    Mutator,
    Let,
    If,
    Else,
    For,
    In,
    Await,
    Inherit,
    Match,
    Return,

    // Operators
    Plus,      // +
    Minus,     // -
    Star,      // *
    Slash,     // /
    Percent,   // %
    Eq,        // ==
    Ne,        // !=
    Lt,        // <
    Le,        // <=
    Gt,        // >
    Ge,        // >=
    And,       // &&
    Or,        // ||
    Not,       // !
    Question,  // ?
    Dot,       // .
    DotDotDot, // ...
    Assign,    // =

    // Delimiters
    LParen,      // (
    RParen,      // )
    LBrace,      // {
    RBrace,      // }
    LBracket,    // [
    RBracket,    // ]
    Comma,       // ,
    Colon,       // :
    Semicolon,   // ;
    FilterStart, // /
    FilterEnd,   // /
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Int(n) => write!(f, "{}", n),
            Token::Float(n) => write!(f, "{}", n),
            Token::String(s) => write!(f, "\"{}\"", s),
            Token::Bool(b) => write!(f, "{}", b),
            Token::UnitLiteral { value, unit } => write!(f, "{}{}", value, unit),
            Token::Ident(s) => write!(f, "{}", s),
            Token::Observer => write!(f, "observer"),
            Token::Mutator => write!(f, "mutator"),
            Token::Let => write!(f, "let"),
            Token::If => write!(f, "if"),
            Token::Else => write!(f, "else"),
            Token::For => write!(f, "for"),
            Token::In => write!(f, "in"),
            Token::Await => write!(f, "await"),
            Token::Inherit => write!(f, "inherit"),
            Token::Match => write!(f, "match"),
            Token::Return => write!(f, "return"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::Eq => write!(f, "=="),
            Token::Ne => write!(f, "!="),
            Token::Lt => write!(f, "<"),
            Token::Le => write!(f, "<="),
            Token::Gt => write!(f, ">"),
            Token::Ge => write!(f, ">="),
            Token::And => write!(f, "&&"),
            Token::Or => write!(f, "||"),
            Token::Not => write!(f, "!"),
            Token::Question => write!(f, "?"),
            Token::Dot => write!(f, "."),
            Token::DotDotDot => write!(f, "..."),
            Token::Assign => write!(f, "="),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::Comma => write!(f, ","),
            Token::Colon => write!(f, ":"),
            Token::Semicolon => write!(f, ";"),
            Token::FilterStart => write!(f, "/"),
            Token::FilterEnd => write!(f, "/"),
        }
    }
}

/// Parse a unit suffix and return the corresponding unit type.
fn parse_unit_suffix(suffix: &str) -> Option<UnitType> {
    match suffix {
        "s" | "seconds" => Some(UnitType::Seconds),
        "min" | "minutes" => Some(UnitType::Minutes),
        "h" | "hours" => Some(UnitType::Hours),
        "d" | "days" => Some(UnitType::Days),
        "deg" | "degrees" => Some(UnitType::Degrees),
        "rad" | "radians" => Some(UnitType::Radians),
        "c" | "celsius" => Some(UnitType::Celsius),
        "f" | "fahrenheit" => Some(UnitType::Fahrenheit),
        "k" | "kelvin" => Some(UnitType::Kelvin),
        _ => None,
    }
}

/// Build the lexer for the automations language.
pub fn lexer<'a>() -> impl Parser<'a, &'a str, Vec<(Token, Span)>, extra::Err<Rich<'a, char>>> {
    // Integer literal (no sign, sign handled in parser as unary op)
    let int = text::int(10)
        .to_slice()
        .map(|s: &str| Token::Int(s.parse().unwrap()))
        .labelled("integer");

    // Float literal (no sign)
    let frac = just('.').then(text::digits(10));
    let float = text::int(10)
        .then(frac)
        .to_slice()
        .map(|s: &str| Token::Float(s.to_string()))
        .labelled("float");

    // Unit literal: number followed immediately by unit suffix
    let unit_literal = text::int(10)
        .then(just('.').ignore_then(text::digits(10).to_slice()).or_not())
        .then(text::ident())
        .try_map(
            |((int_part, frac_part), suffix): ((&str, Option<&str>), &str), span| {
                let value = if let Some(frac) = frac_part {
                    format!("{}.{}", int_part, frac)
                } else {
                    int_part.to_string()
                };

                parse_unit_suffix(suffix)
                    .map(|unit| Token::UnitLiteral { value, unit })
                    .ok_or_else(|| Rich::custom(span, format!("invalid unit suffix: {}", suffix)))
            },
        )
        .labelled("unit literal");

    // String literal with escape sequences
    let escape = just('\\').ignore_then(choice((
        just('\\').to('\\'),
        just('"').to('"'),
        just('n').to('\n'),
        just('r').to('\r'),
        just('t').to('\t'),
    )));

    let string = none_of("\\\"")
        .or(escape)
        .repeated()
        .collect::<String>()
        .delimited_by(just('"'), just('"'))
        .map(Token::String)
        .labelled("string");

    // Keywords and identifiers
    let ident = text::ident().map(|s: &str| match s {
        "observer" => Token::Observer,
        "mutator" => Token::Mutator,
        "let" => Token::Let,
        "if" => Token::If,
        "else" => Token::Else,
        "for" => Token::For,
        "in" => Token::In,
        "await" => Token::Await,
        "inherit" => Token::Inherit,
        "match" => Token::Match,
        "return" => Token::Return,
        "true" => Token::Bool(true),
        "false" => Token::Bool(false),
        _ => Token::Ident(s.to_string()),
    });

    // Operators (order matters for multi-char ops)
    let op = choice((
        just("...").to(Token::DotDotDot),
        just("==").to(Token::Eq),
        just("!=").to(Token::Ne),
        just("<=").to(Token::Le),
        just(">=").to(Token::Ge),
        just("&&").to(Token::And),
        just("||").to(Token::Or),
        just("<").to(Token::Lt),
        just(">").to(Token::Gt),
        just("!").to(Token::Not),
        just("+").to(Token::Plus),
        just("-").to(Token::Minus),
        just("*").to(Token::Star),
        just("/").to(Token::Slash),
        just("%").to(Token::Percent),
        just("?").to(Token::Question),
        just(".").to(Token::Dot),
        just("=").to(Token::Assign),
    ));

    // Delimiters
    let delim = choice((
        just("(").to(Token::LParen),
        just(")").to(Token::RParen),
        just("{").to(Token::LBrace),
        just("}").to(Token::RBrace),
        just("[").to(Token::LBracket),
        just("]").to(Token::RBracket),
        just(",").to(Token::Comma),
        just(":").to(Token::Colon),
        just(";").to(Token::Semicolon),
    ));

    // Token: try unit literal first, then float, then int, to avoid ambiguity
    let token = choice((unit_literal, float, int, string, ident, op, delim));

    token
        .map_with(|tok, e: &mut MapExtra<'a, '_, &'a str, _>| (tok, e.span()))
        .padded()
        .repeated()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_lex_comments() {
        // Comments will be handled by the parser, not lexer for now
        let input = "42 foo";
        let result = lexer().parse(input).into_result().unwrap();
        assert_eq!(
            result,
            vec![
                (Token::Int(42), (0..2).into()),
                (Token::Ident("foo".to_string()), (3..6).into()),
            ]
        );
    }
}
