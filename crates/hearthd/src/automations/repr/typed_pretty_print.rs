//! Verbose, multi-line pretty-printing for typed AST nodes.
//!
//! Used by type checker tests to produce unambiguous snapshot output.
//! Shows `[type: X]` annotations on every expression.

use super::pretty_print::PrettyPrint;
use super::pretty_print::write_indent;
use super::typed::*;

impl PrettyPrint for TypedExpr {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match &self.kind {
            TypedExprKind::Int(n) => writeln!(f, "Int: {} [type: {}]", n, self.ty),
            TypedExprKind::Float(n) => writeln!(f, "Float: {} [type: {}]", n, self.ty),
            TypedExprKind::String(s) => writeln!(f, "String: \"{}\" [type: {}]", s, self.ty),
            TypedExprKind::Bool(b) => writeln!(f, "Bool: {} [type: {}]", b, self.ty),
            TypedExprKind::UnitLiteral { value, unit } => {
                writeln!(f, "UnitLiteral: {}{} [type: {}]", value, unit, self.ty)
            }
            TypedExprKind::Ident(s) => writeln!(f, "Ident: {} [type: {}]", s, self.ty),
            TypedExprKind::Path(segments) => {
                writeln!(f, "Path: [type: {}]", self.ty)?;
                for seg in segments {
                    write_indent(indent + 1, f)?;
                    writeln!(f, "Segment: {}", seg)?;
                }
                Ok(())
            }
            TypedExprKind::BinOp { op, left, right } => {
                writeln!(f, "BinOp: {} [type: {}]", op, self.ty)?;
                left.pretty_print(indent + 1, f)?;
                right.pretty_print(indent + 1, f)
            }
            TypedExprKind::UnaryOp { op, expr } => {
                writeln!(f, "UnaryOp: {} [type: {}]", op, self.ty)?;
                expr.pretty_print(indent + 1, f)
            }
            TypedExprKind::Field { expr, field } => {
                writeln!(f, "Field: .{} [type: {}]", field, self.ty)?;
                expr.pretty_print(indent + 1, f)
            }
            TypedExprKind::OptionalField { expr, field } => {
                writeln!(f, "OptionalField: ?.{} [type: {}]", field, self.ty)?;
                expr.pretty_print(indent + 1, f)
            }
            TypedExprKind::Call { func, args } => {
                writeln!(f, "Call: [type: {}]", self.ty)?;
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
            TypedExprKind::If {
                cond,
                then_block,
                else_block,
            } => {
                writeln!(f, "If: [type: {}]", self.ty)?;
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
            TypedExprKind::List(items) => {
                if items.is_empty() {
                    writeln!(f, "List: (empty) [type: {}]", self.ty)
                } else {
                    writeln!(f, "List: [type: {}]", self.ty)?;
                    for item in items {
                        item.pretty_print(indent + 1, f)?;
                    }
                    Ok(())
                }
            }
            TypedExprKind::StructLit { name, fields } => {
                writeln!(f, "StructLit: {} [type: {}]", name, self.ty)?;
                for field in fields {
                    field.pretty_print(indent + 1, f)?;
                }
                Ok(())
            }
            TypedExprKind::Block { stmts, result } => {
                writeln!(f, "Block: [type: {}]", self.ty)?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Stmts:")?;
                for stmt in stmts {
                    stmt.pretty_print(indent + 2, f)?;
                }
                write_indent(indent + 1, f)?;
                writeln!(f, "Result:")?;
                result.pretty_print(indent + 2, f)
            }
            TypedExprKind::MutableList => writeln!(f, "MutableList [type: {}]", self.ty),
        }
    }
}

impl PrettyPrint for TypedStmt {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            TypedStmt::Let { name, value, .. } => {
                writeln!(f, "Let: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
            TypedStmt::LetMut { name, value, .. } => {
                writeln!(f, "LetMut: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
            TypedStmt::Expr(expr) => {
                writeln!(f, "ExprStmt:")?;
                expr.pretty_print(indent + 1, f)
            }
            TypedStmt::Return(expr, _) => {
                writeln!(f, "Return:")?;
                expr.pretty_print(indent + 1, f)
            }
            TypedStmt::For {
                var, iter, body, ..
            } => {
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
            TypedStmt::Push { list, value, .. } => {
                writeln!(f, "Push: {}", list)?;
                value.pretty_print(indent + 1, f)
            }
        }
    }
}

impl PrettyPrint for TypedArg {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypedArg::Positional(expr) => expr.pretty_print(indent, f),
            TypedArg::Named { name, value } => {
                write_indent(indent, f)?;
                writeln!(f, "Named: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
        }
    }
}

impl PrettyPrint for TypedStructField {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            TypedStructField::Field { name, value } => {
                writeln!(f, "Field: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
            TypedStructField::Inherit(name) => writeln!(f, "Inherit: {}", name),
            TypedStructField::Spread(name) => writeln!(f, "Spread: {}", name),
        }
    }
}

impl PrettyPrint for TypedAutomation {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        writeln!(f, "Automation: {}", self.kind)?;
        write_indent(indent + 1, f)?;
        writeln!(f, "Pattern:")?;
        self.pattern.pretty_print(indent + 2, f)?;
        if let Some(filter) = &self.filter {
            write_indent(indent + 1, f)?;
            writeln!(f, "Filter:")?;
            filter.pretty_print(indent + 2, f)?;
        }
        write_indent(indent + 1, f)?;
        if self.body.is_empty() {
            writeln!(f, "Body: (empty)")
        } else {
            writeln!(f, "Body:")?;
            for stmt in &self.body {
                stmt.pretty_print(indent + 2, f)?;
            }
            Ok(())
        }
    }
}

impl PrettyPrint for TypedProgram {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypedProgram::Automation(auto) => auto.pretty_print(indent, f),
            TypedProgram::Template {
                params,
                automations,
            } => {
                write_indent(indent, f)?;
                writeln!(f, "Template:")?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Params:")?;
                for param in params {
                    param.pretty_print(indent + 2, f)?;
                }
                write_indent(indent + 1, f)?;
                writeln!(f, "Automations:")?;
                for auto in automations {
                    auto.pretty_print(indent + 2, f)?;
                }
                Ok(())
            }
        }
    }
}

impl PrettyPrint for CheckResult {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.program.pretty_print(indent, f)?;
        if !self.constraints.is_empty() {
            write_indent(indent, f)?;
            writeln!(f, "EntityConstraints:")?;
            for c in &self.constraints {
                write_indent(indent + 1, f)?;
                writeln!(
                    f,
                    "{}.{} @ {}..{}",
                    c.domain, c.entity, c.span.start, c.span.end
                )?;
            }
        }
        if !self.errors.is_empty() {
            write_indent(indent, f)?;
            writeln!(f, "Errors:")?;
            for e in &self.errors {
                write_indent(indent + 1, f)?;
                writeln!(f, "{}", e)?;
            }
        }
        Ok(())
    }
}
