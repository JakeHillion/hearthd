//! Verbose, multi-line pretty-printing for AST nodes.
//!
//! Used by parser tests to produce unambiguous snapshot output.

use super::ast::*;

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

impl<T: PrettyPrint> PrettyPrint for Spanned<T> {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.node.pretty_print(indent, f)
    }
}

impl PrettyPrint for Expr {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            Expr::Int(n) => writeln!(f, "Int: {}", n),
            Expr::Float(n) => writeln!(f, "Float: {}", n),
            Expr::String(s) => writeln!(f, "String: \"{}\"", s),
            Expr::Bool(b) => writeln!(f, "Bool: {}", b),
            Expr::UnitLiteral { value, unit } => writeln!(f, "UnitLiteral: {}{}", value, unit),
            Expr::Ident(s) => writeln!(f, "Ident: {}", s),
            Expr::Path(segments) => {
                writeln!(f, "Path:")?;
                for seg in segments {
                    write_indent(indent + 1, f)?;
                    writeln!(f, "Segment: {}", seg)?;
                }
                Ok(())
            }
            Expr::BinOp { op, left, right } => {
                writeln!(f, "BinOp: {}", op)?;
                left.pretty_print(indent + 1, f)?;
                right.pretty_print(indent + 1, f)
            }
            Expr::UnaryOp { op, expr } => {
                writeln!(f, "UnaryOp: {}", op)?;
                expr.pretty_print(indent + 1, f)
            }
            Expr::Field { expr, field } => {
                writeln!(f, "Field: .{}", field)?;
                expr.pretty_print(indent + 1, f)
            }
            Expr::OptionalField { expr, field } => {
                writeln!(f, "OptionalField: ?.{}", field)?;
                expr.pretty_print(indent + 1, f)
            }
            Expr::Call { func, args } => {
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
            Expr::If {
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
            Expr::List(items) => {
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
            Expr::ListComp {
                expr,
                var,
                iter,
                filter,
            } => {
                writeln!(f, "ListComp:")?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Expr:")?;
                expr.pretty_print(indent + 2, f)?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Var: {}", var)?;
                write_indent(indent + 1, f)?;
                writeln!(f, "Iter:")?;
                iter.pretty_print(indent + 2, f)?;
                if let Some(filt) = filter {
                    write_indent(indent + 1, f)?;
                    writeln!(f, "Filter:")?;
                    filt.pretty_print(indent + 2, f)?;
                }
                Ok(())
            }
            Expr::StructLit { name, fields } => {
                writeln!(f, "StructLit: {}", name)?;
                for field in fields {
                    field.pretty_print(indent + 1, f)?;
                }
                Ok(())
            }
        }
    }
}

impl PrettyPrint for Stmt {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            Stmt::Let { name, value } => {
                writeln!(f, "Let: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
            Stmt::Expr(expr) => {
                writeln!(f, "ExprStmt:")?;
                expr.pretty_print(indent + 1, f)
            }
            Stmt::Return(expr) => {
                writeln!(f, "Return:")?;
                expr.pretty_print(indent + 1, f)
            }
        }
    }
}

impl PrettyPrint for Arg {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Arg::Positional(expr) => expr.pretty_print(indent, f),
            Arg::Named { name, value } => {
                write_indent(indent, f)?;
                writeln!(f, "Named: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
        }
    }
}

impl PrettyPrint for StructField {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            StructField::Field { name, value } => {
                writeln!(f, "Field: {}", name)?;
                value.pretty_print(indent + 1, f)
            }
            StructField::Inherit(name) => writeln!(f, "Inherit: {}", name),
            StructField::Spread(name) => writeln!(f, "Spread: {}", name),
        }
    }
}

impl PrettyPrint for Pattern {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            Pattern::Ident(s) => writeln!(f, "PatternIdent: {}", s),
            Pattern::Struct { fields, has_rest } => {
                writeln!(f, "PatternStruct:")?;
                for field in fields {
                    field.pretty_print(indent + 1, f)?;
                }
                if *has_rest {
                    write_indent(indent + 1, f)?;
                    writeln!(f, "Rest: ...")?;
                }
                Ok(())
            }
        }
    }
}

impl PrettyPrint for FieldPattern {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        if let Some(pattern) = &self.pattern {
            writeln!(f, "FieldPattern: {}", self.name)?;
            pattern.pretty_print(indent + 1, f)
        } else {
            writeln!(f, "FieldPattern: {}", self.name)
        }
    }
}

impl PrettyPrint for Automation {
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

impl PrettyPrint for Program {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Program::Automation(auto) => auto.pretty_print(indent, f),
            Program::Template(tmpl) => tmpl.pretty_print(indent, f),
        }
    }
}

impl PrettyPrint for Template {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        writeln!(f, "Template:")?;
        write_indent(indent + 1, f)?;
        writeln!(f, "Params:")?;
        for param in &self.params {
            param.pretty_print(indent + 2, f)?;
        }
        write_indent(indent + 1, f)?;
        writeln!(f, "Automations:")?;
        for auto in &self.automations {
            auto.pretty_print(indent + 2, f)?;
        }
        Ok(())
    }
}

impl PrettyPrint for TemplateParam {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        write!(f, "Param: {}: ", self.name)?;
        write_type_inline(&self.ty, f)?;
        writeln!(f)
    }
}

fn write_type_inline(ty: &Type, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match ty {
        Type::Named(s) => write!(f, "{}", s),
        Type::List(t) => {
            write!(f, "[")?;
            write_type_inline(t, f)?;
            write!(f, "]")
        }
        Type::Set(t) => {
            write!(f, "Set<")?;
            write_type_inline(t, f)?;
            write!(f, ">")
        }
        Type::Map { key, value } => {
            write!(f, "Map<")?;
            write_type_inline(key, f)?;
            write!(f, ", ")?;
            write_type_inline(value, f)?;
            write!(f, ">")
        }
        Type::Option(t) => {
            write!(f, "Option<")?;
            write_type_inline(t, f)?;
            write!(f, ">")
        }
    }
}
