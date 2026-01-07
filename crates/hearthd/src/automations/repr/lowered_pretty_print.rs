//! Verbose, multi-line pretty-printing for lowered AST nodes.
//!
//! Used by desugar tests to produce unambiguous snapshot output.

use super::lowered::*;

/// Trait for verbose, multi-line AST pretty-printing.
pub trait PrettyPrint {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;

    fn to_pretty_string(&self) -> String {
        struct Wrapper<'a, T: PrettyPrint + ?Sized>(&'a T);
        impl<T: PrettyPrint + ?Sized> std::fmt::Display for Wrapper<'_, T> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.pretty_print(0, f)
            }
        }
        Wrapper(self).to_string()
    }
}

fn write_indent(indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    for _ in 0..indent {
        write!(f, "  ")?;
    }
    Ok(())
}

impl PrettyPrint for Origin {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            Origin::Direct(rc) => {
                let span = rc.span;
                writeln!(f, "Origin: Direct @ {}..{}", span.start, span.end)
            }
            Origin::ListComp(rc) => {
                let span = rc.span;
                writeln!(f, "Origin: ListComp @ {}..{}", span.start, span.end)
            }
        }
    }
}

impl<T: PrettyPrint> PrettyPrint for Spanned<T> {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.origin.pretty_print(indent, f)?;
        self.node.pretty_print(indent, f)
    }
}

impl PrettyPrint for LoweredExpr {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            LoweredExpr::Int(n) => writeln!(f, "Int: {}", n),
            LoweredExpr::Float(n) => writeln!(f, "Float: {}", n),
            LoweredExpr::String(s) => writeln!(f, "String: \"{}\"", s),
            LoweredExpr::Bool(b) => writeln!(f, "Bool: {}", b),
            LoweredExpr::UnitLiteral { value, unit } => {
                writeln!(f, "UnitLiteral: {}{}", value, unit)
            }
            LoweredExpr::Ident(s) => writeln!(f, "Ident: {}", s),
            LoweredExpr::Path(segments) => {
                writeln!(f, "Path:")?;
                for seg in segments {
                    write_indent(indent + 1, f)?;
                    writeln!(f, "Segment: {}", seg)?;
                }
                Ok(())
            }
            LoweredExpr::BinOp { op, left, right } => {
                writeln!(f, "BinOp: {}", op)?;
                left.pretty_print(indent + 1, f)?;
                right.pretty_print(indent + 1, f)
            }
            LoweredExpr::UnaryOp { op, expr } => {
                writeln!(f, "UnaryOp: {}", op)?;
                expr.pretty_print(indent + 1, f)
            }
            LoweredExpr::Field { expr, field } => {
                writeln!(f, "Field: .{}", field)?;
                expr.pretty_print(indent + 1, f)
            }
            LoweredExpr::OptionalField { expr, field } => {
                writeln!(f, "OptionalField: ?.{}", field)?;
                expr.pretty_print(indent + 1, f)
            }
            LoweredExpr::Call { func, args } => {
                writeln!(f, "Call:")?;
                func.pretty_print(indent + 1, f)?;
                write_indent(indent + 1, f)?;
                if args.is_empty() {
                    writeln!(f, "Args: (none)")
                } else {
                    writeln!(f, "Args:")?;
                    for arg in args {
                        arg.pretty_print(indent + 2, f)?;
                    }
                    Ok(())
                }
            }
            LoweredExpr::If {
                cond,
                then_block,
                else_block,
            } => {
                writeln!(f, "If:")?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Cond:")?;
                cond.pretty_print(indent + 2, f)?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Then:")?;
                for stmt in then_block {
                    stmt.pretty_print(indent + 2, f)?;
                }
                if let Some(else_stmts) = else_block {
                    write_indent(indent + 1, f)?;
                    writeln!(f, "Else:")?;
                    for stmt in else_stmts {
                        stmt.pretty_print(indent + 2, f)?;
                    }
                }
                Ok(())
            }
            LoweredExpr::List(items) => {
                if items.is_empty() {
                    writeln!(f, "List: (empty)")
                } else {
                    writeln!(f, "List:")?;
                    for item in items {
                        item.pretty_print(indent + 1, f)?;
                    }
                    Ok(())
                }
            }
            LoweredExpr::StructLit { name, fields } => {
                writeln!(f, "StructLit: {}", name)?;
                for field in fields {
                    field.pretty_print(indent + 1, f)?;
                }
                Ok(())
            }
            LoweredExpr::Block { stmts, result } => {
                writeln!(f, "Block:")?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Stmts:")?;
                for stmt in stmts {
                    stmt.pretty_print(indent + 2, f)?;
                }
                write_indent(indent + 1, f)?;
                writeln!(f, "Result:")?;
                result.pretty_print(indent + 2, f)
            }
            LoweredExpr::MutableList => writeln!(f, "MutableList"),
        }
    }
}

impl PrettyPrint for LoweredStmt {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            LoweredStmt::Let { name, value } => {
                writeln!(f, "Let: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
            LoweredStmt::LetMut { name, value } => {
                writeln!(f, "LetMut: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
            LoweredStmt::Expr(expr) => {
                writeln!(f, "ExprStmt:")?;
                expr.pretty_print(indent + 1, f)
            }
            LoweredStmt::Return(expr) => {
                writeln!(f, "Return:")?;
                expr.pretty_print(indent + 1, f)
            }
            LoweredStmt::For { var, iter, body } => {
                writeln!(f, "For:")?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Var: {}", var)?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Iter:")?;
                iter.pretty_print(indent + 2, f)?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Body:")?;
                for stmt in body {
                    stmt.pretty_print(indent + 2, f)?;
                }
                Ok(())
            }
            LoweredStmt::Push { list, value } => {
                writeln!(f, "Push: {}", list)?;
                value.pretty_print(indent + 1, f)
            }
        }
    }
}

impl PrettyPrint for LoweredArg {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoweredArg::Positional(expr) => expr.pretty_print(indent, f),
            LoweredArg::Named { name, value } => {
                write_indent(indent, f)?;
                writeln!(f, "Named: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
        }
    }
}

impl PrettyPrint for LoweredStructField {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            LoweredStructField::Field { name, value } => {
                writeln!(f, "Field: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
            LoweredStructField::Inherit(name) => writeln!(f, "Inherit: {}", name),
            LoweredStructField::Spread(name) => writeln!(f, "Spread: {}", name),
        }
    }
}
