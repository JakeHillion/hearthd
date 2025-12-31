//! Parser for the HearthD Automations language.

use chumsky::prelude::*;
use chumsky::span::SimpleSpan;

use crate::automations::ast::*;
use crate::automations::lexer::Token;

/// Parse a complete automation program.
pub fn parse(input: &str) -> Result<Spanned<Program>, Vec<Rich<'static, Token>>> {
    let tokens = crate::automations::lexer::lexer()
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
pub(crate) fn expr_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<Expr>, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: chumsky::input::ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    // Helper enum for postfix operations
    enum PostfixOp {
        Call(Vec<Spanned<Arg>>),
        Field(String),
        OptionalField(String),
    }

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

        // Struct literal: Name { fields }
        let struct_field = choice((
            // Field: field: value
            select! { Token::Ident(s) => s }
                .then_ignore(just(Token::Colon))
                .then(expr.clone())
                .map_with(|(name, value), e| {
                    Spanned::new(StructField::Field { name, value }, e.span())
                }),
            // Inherit: inherit field
            just(Token::Inherit)
                .ignore_then(select! { Token::Ident(s) => s })
                .map_with(|name, e| Spanned::new(StructField::Inherit(name), e.span())),
            // Spread: ...name
            just(Token::DotDotDot)
                .ignore_then(select! { Token::Ident(s) => s })
                .map_with(|name, e| Spanned::new(StructField::Spread(name), e.span())),
        ));

        let struct_lit = select! { Token::Ident(s) => s }
            .then(
                struct_field
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map(|(name, fields)| Expr::StructLit { name, fields });

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

        // Block of statements (reusable for if branches)
        // Uses stmt_parser_with to pass the recursive expr reference
        let block = stmt_parser_with(expr.clone())
            .repeated()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace));

        // If expression
        let if_expr = just(Token::If)
            .ignore_then(expr.clone())
            .then(block.clone())
            .then_ignore(just(Token::Else))
            .then(block)
            .map(|((cond, then_block), else_block)| Expr::If {
                cond: Box::new(cond),
                then_block,
                else_block,
            });

        let atom = choice((literal, struct_lit, ident, list, if_expr, paren_expr))
            .map_with(|node, e| Spanned::new(node, e.span()))
            .boxed();

        // Function argument: either `name = expr` (named) or `expr` (positional)
        let arg = choice((
            // Named: ident = expr (per design doc: `wait(5 minutes, retry = cancel)`)
            select! { Token::Ident(s) => s }
                .then_ignore(just(Token::Assign))
                .then(expr.clone())
                .map_with(|(name, value), e| Spanned::new(Arg::Named { name, value }, e.span())),
            // Positional: expr
            expr.clone()
                .map_with(|value, e| Spanned::new(Arg::Positional(value), e.span())),
        ));

        // Field access and function calls
        let call = atom.clone().foldl_with(
            choice((
                // Function call: (args)
                arg.separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .map(PostfixOp::Call),
                // Field access: .field
                just(Token::Dot)
                    .ignore_then(select! { Token::Ident(s) => s })
                    .map(PostfixOp::Field),
                // Optional chaining: ?.field
                just(Token::Question)
                    .then(just(Token::Dot))
                    .ignore_then(select! { Token::Ident(s) => s })
                    .map(PostfixOp::OptionalField),
            ))
            .repeated(),
            |expr, op, e| {
                let node = match op {
                    PostfixOp::Call(args) => Expr::Call {
                        func: Box::new(expr),
                        args,
                    },
                    PostfixOp::Field(field) => Expr::Field {
                        expr: Box::new(expr),
                        field,
                    },
                    PostfixOp::OptionalField(field) => Expr::OptionalField {
                        expr: Box::new(expr),
                        field,
                    },
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

        // Comparison: <, >, <=, >=, in
        let cmp_op = select! {
            Token::Lt => BinOp::Lt,
            Token::Le => BinOp::Le,
            Token::Gt => BinOp::Gt,
            Token::Ge => BinOp::Ge,
            Token::In => BinOp::In,
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

/// Parser for statements, parameterized by an expression parser.
///
/// This allows breaking mutual recursion between expr_parser and stmt_parser
/// by passing the recursive expression reference from within expr_parser.
fn stmt_parser_with<'tokens, 'src: 'tokens, I, E>(
    expr: E,
) -> impl Parser<'tokens, I, Spanned<Stmt>, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: chumsky::input::ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
    E: Parser<'tokens, I, Spanned<Expr>, extra::Err<Rich<'tokens, Token>>> + Clone,
{
    let let_stmt = just(Token::Let)
        .ignore_then(select! { Token::Ident(s) => s })
        .then_ignore(just(Token::Assign))
        .then(expr.clone())
        .then_ignore(just(Token::Semicolon))
        .map_with(|(name, value), e| Spanned::new(Stmt::Let { name, value }, e.span()));

    let expr_stmt = expr
        .then(just(Token::Semicolon).or_not())
        .map_with(|(expr, _), e| Spanned::new(Stmt::Expr(expr), e.span()));

    choice((let_stmt, expr_stmt))
}

/// Parser for statements using the top-level expression parser.
fn stmt_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<Stmt>, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: chumsky::input::ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    stmt_parser_with(expr_parser())
}

/// Automation parser - parses `observer {} /filter/ { stmts }`
///
/// Pattern is currently stubbed to empty braces; filter and body are fully parsed.
fn automation_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<Automation>, extra::Err<Rich<'tokens, Token>>>
where
    I: chumsky::input::ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    let kind = select! {
        Token::Observer => AutomationKind::Observer,
        Token::Mutator => AutomationKind::Mutator,
    };

    // Pattern parser for struct destructuring (recursive for nested patterns)
    let pattern = recursive(|pattern| {
        let field_pattern = select! { Token::Ident(s) => s }
            .then(just(Token::Colon).ignore_then(pattern).or_not())
            .map_with(|(name, nested), e| {
                Spanned::new(
                    FieldPattern {
                        name,
                        pattern: nested,
                    },
                    e.span(),
                )
            });

        let rest = just(Token::DotDotDot).to(true);

        just(Token::LBrace)
            .ignore_then(
                field_pattern
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .then(rest.or_not()),
            )
            .then_ignore(just(Token::RBrace))
            .map_with(|(fields, has_rest), e| {
                Spanned::new(
                    Pattern::Struct {
                        fields,
                        has_rest: has_rest.unwrap_or(false),
                    },
                    e.span(),
                )
            })
    });

    // Filter uses expr_parser
    let filter = just(Token::Slash)
        .ignore_then(expr_parser())
        .then_ignore(just(Token::Slash));

    // Body - list of statements
    let body = stmt_parser()
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBrace), just(Token::RBrace));

    kind.then(pattern)
        .then(filter)
        .then(body)
        .map_with(|(((kind, pattern), filter), body), e| {
            Spanned::new(
                Automation {
                    kind,
                    pattern,
                    filter,
                    body,
                },
                e.span(),
            )
        })
}
