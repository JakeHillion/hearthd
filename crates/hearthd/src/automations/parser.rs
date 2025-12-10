//! Parser for the HearthD Automations language.

use chumsky::prelude::*;

use super::ast::*;
use super::lexer::Token;

/// Parse a complete automation program.
#[allow(dead_code)]
pub fn parse(input: &str) -> Result<Spanned<Program>, Vec<Rich<'static, Token>>> {
    let tokens = super::lexer::lexer()
        .parse(input)
        .into_result()
        .map_err(|errs| {
            errs.into_iter()
                .map(|err| Rich::<Token>::custom(*err.span(), format!("Lexer error: {}", err)))
                .collect::<Vec<_>>()
        })?;
    let input_len = input.len();
    let result = automation_parser()
        .parse(
            tokens
                .as_slice()
                .map((input_len..input_len).into(), |(t, s)| (t, s)),
        )
        .into_result()
        .map(|auto| Spanned::new(Program::Automation(auto.node), auto.span))
        .map_err(|errs| errs.into_iter().map(|e| e.into_owned()).collect());
    result
}

/// Parser for expressions.
#[allow(dead_code)]
fn expr_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<Expr>, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: chumsky::input::ValueInput<'tokens, Token = Token, Span = Span>,
{
    recursive(|expr| {
        // Primary expressions
        let literal = select! {
            Token::Int(n) => Expr::Int(n),
            Token::Float(f) => Expr::Float(f),
            Token::String(s) => Expr::String(s),
            Token::Bool(b) => Expr::Bool(b),
            Token::UnitLiteral { value, unit } => Expr::UnitLiteral { value, unit },
        }
        .labelled("literal");

        let ident = select! {
            Token::Ident(s) => Expr::Ident(s),
        }
        .labelled("identifier");

        let list = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(Expr::List)
            .labelled("list");

        let paren_expr = expr
            .clone()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(|e| e.node);

        let atom = choice((literal, ident, list, paren_expr))
            .map_with(|node, e| Spanned::new(node, e.span()))
            .boxed();

        // Field access and function calls
        let call = atom.clone().foldl_with(
            choice((
                // Field access: .field
                just(Token::Dot)
                    .ignore_then(select! { Token::Ident(s) => s })
                    .map(|field| (field, false)),
                // Optional chaining: ?.field
                just(Token::Question)
                    .then(just(Token::Dot))
                    .ignore_then(select! { Token::Ident(s) => s })
                    .map(|field| (field, true)),
            ))
            .repeated(),
            |expr, (field, is_optional), e| {
                let node = if is_optional {
                    Expr::OptionalField {
                        expr: Box::new(expr),
                        field,
                    }
                } else {
                    Expr::Field {
                        expr: Box::new(expr),
                        field,
                    }
                };
                Spanned::new(node, e.span())
            },
        );

        // Unary operators
        let unary_op = select! {
            Token::Not => UnaryOp::Not,
            Token::Minus => UnaryOp::Neg,
            Token::Star => UnaryOp::Deref,
            Token::Await => UnaryOp::Await,
        };

        let unary = unary_op.repeated().foldr_with(call, |op, expr, e| {
            Spanned::new(
                Expr::UnaryOp {
                    op,
                    expr: Box::new(expr),
                },
                e.span(),
            )
        });

        // Multiplicative: *, /, %
        let mul_op = select! {
            Token::Star => BinOp::Mul,
            Token::Slash => BinOp::Div,
            Token::Percent => BinOp::Mod,
        };

        let mul =
            unary
                .clone()
                .foldl_with(mul_op.then(unary).repeated(), |left, (op, right), e| {
                    Spanned::new(
                        Expr::BinOp {
                            op,
                            left: Box::new(left),
                            right: Box::new(right),
                        },
                        e.span(),
                    )
                });

        // Additive: +, -
        let add_op = select! {
            Token::Plus => BinOp::Add,
            Token::Minus => BinOp::Sub,
        };

        let add = mul
            .clone()
            .foldl_with(add_op.then(mul).repeated(), |left, (op, right), e| {
                Spanned::new(
                    Expr::BinOp {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    e.span(),
                )
            });

        // Comparison: <, >, <=, >=
        let cmp_op = select! {
            Token::Lt => BinOp::Lt,
            Token::Le => BinOp::Le,
            Token::Gt => BinOp::Gt,
            Token::Ge => BinOp::Ge,
        };

        let cmp = add
            .clone()
            .foldl_with(cmp_op.then(add).repeated(), |left, (op, right), e| {
                Spanned::new(
                    Expr::BinOp {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    e.span(),
                )
            });

        // Equality: ==, !=
        let eq_op = select! {
            Token::Eq => BinOp::Eq,
            Token::Ne => BinOp::Ne,
        };

        let eq = cmp
            .clone()
            .foldl_with(eq_op.then(cmp).repeated(), |left, (op, right), e| {
                Spanned::new(
                    Expr::BinOp {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    e.span(),
                )
            });

        // Logical AND: &&
        let and_op = select! { Token::And => BinOp::And };

        let and = eq
            .clone()
            .foldl_with(and_op.then(eq).repeated(), |left, (op, right), e| {
                Spanned::new(
                    Expr::BinOp {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    e.span(),
                )
            });

        // Logical OR: ||
        let or_op = select! { Token::Or => BinOp::Or };

        and.clone()
            .foldl_with(or_op.then(and).repeated(), |left, (op, right), e| {
                Spanned::new(
                    Expr::BinOp {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    e.span(),
                )
            })
    })
}

/// Stub automation parser - to be implemented
fn automation_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<Automation>, extra::Err<Rich<'tokens, Token>>>
where
    I: chumsky::input::ValueInput<'tokens, Token = Token, Span = Span>,
{
    // Stub implementation - just fail with an error
    select! {
        Token::Observer => (),
        Token::Mutator => (),
    }
    .try_map(|_, span| Err(Rich::custom(span, "automation parser not yet implemented")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_expr(input: &str) -> Result<Spanned<Expr>, Vec<Rich<'static, Token>>> {
        let tokens = super::super::lexer::lexer()
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
}
