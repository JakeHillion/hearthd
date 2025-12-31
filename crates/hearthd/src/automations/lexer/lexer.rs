//! Lexer and token definitions for the HearthD Automations language.

use chumsky::input::MapExtra;
use chumsky::prelude::*;
use chumsky::span::SimpleSpan;

use crate::automations::ast::UnitType;

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
    ColonColon,  // ::
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
            Token::ColonColon => write!(f, "::"),
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
pub fn lexer<'a>() -> impl Parser<'a, &'a str, Vec<(Token, SimpleSpan)>, extra::Err<Rich<'a, char>>>
{
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

    // Delimiters (order matters: :: before :)
    let delim = choice((
        just("(").to(Token::LParen),
        just(")").to(Token::RParen),
        just("{").to(Token::LBrace),
        just("}").to(Token::RBrace),
        just("[").to(Token::LBracket),
        just("]").to(Token::RBracket),
        just(",").to(Token::Comma),
        just("::").to(Token::ColonColon),
        just(":").to(Token::Colon),
        just(";").to(Token::Semicolon),
    ));

    // Comments (skipped like whitespace)
    let line_comment = just("//")
        .then(any().and_is(just('\n').not()).repeated())
        .ignored();

    let block_comment = just("/*")
        .then(any().and_is(just("*/").not()).repeated())
        .then(just("*/"))
        .ignored();

    let comment = line_comment.or(block_comment);

    // Whitespace and comments to skip between tokens
    let ws = choice((text::whitespace().at_least(1).ignored(), comment))
        .repeated()
        .ignored();

    // Token: try unit literal first, then float, then int, to avoid ambiguity
    let token = choice((unit_literal, float, int, string, ident, op, delim));

    token
        .map_with(|tok, e: &mut MapExtra<'a, '_, &'a str, _>| (tok, e.span()))
        .padded_by(ws)
        .repeated()
        .collect()
}
