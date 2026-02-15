//! Pretty-printing for HIR basic blocks.
//!
//! Used by lowering tests to produce readable snapshot output.

use super::hir::*;
use super::pretty_print::PrettyPrint;
use super::pretty_print::write_indent;

impl std::fmt::Display for Tmp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "%{}", self.0)
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

impl PrettyPrint for HirProgram {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HirProgram::Automation(auto) => auto.pretty_print(indent, f),
            HirProgram::Template {
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

impl PrettyPrint for HirAutomation {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        writeln!(f, "Automation: {}", self.kind)?;
        if !self.params.is_empty() {
            write_indent(indent + 1, f)?;
            writeln!(f, "Params:")?;
            for param in &self.params {
                write_indent(indent + 2, f)?;
                writeln!(f, "{}: {} [{}]", param.tmp, param.name, param.ty)?;
            }
        }
        self.blocks.pretty_print(indent + 1, f)
    }
}

impl PrettyPrint for Vec<BasicBlock> {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for block in self {
            block.pretty_print(indent, f)?;
        }
        Ok(())
    }
}

impl PrettyPrint for BasicBlock {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        writeln!(f, "{}:", self.id)?;
        for instr in &self.instructions {
            instr.pretty_print(indent + 1, f)?;
        }
        self.terminator.pretty_print(indent + 1, f)
    }
}

impl PrettyPrint for Instruction {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        write!(f, "{} = ", self.dst)?;
        write_op(&self.op, f)?;
        writeln!(f, " [{}]", self.ty)
    }
}

fn write_op(op: &Op, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match op {
        Op::ConstInt(n) => write!(f, "const_int {}", n),
        Op::ConstFloat(n) => write!(f, "const_float {}", n),
        Op::ConstString(s) => write!(f, "const_string \"{}\"", s),
        Op::ConstBool(b) => write!(f, "const_bool {}", b),
        Op::ConstUnit { value, unit } => write!(f, "const_unit {}{}", value, unit),
        Op::Unit => write!(f, "unit"),
        Op::BinOp { op, left, right } => write!(f, "{} {}, {}", op, left, right),
        Op::Neg(tmp) => write!(f, "neg {}", tmp),
        Op::Not(tmp) => write!(f, "not {}", tmp),
        Op::Deref(tmp) => write!(f, "deref {}", tmp),
        Op::Await(tmp) => write!(f, "await {}", tmp),
        Op::Field { base, field } => write!(f, "field {}.{}", base, field),
        Op::OptionalField { base, field } => write!(f, "optional_field {}?.{}", base, field),
        Op::Call { name, args } => {
            write!(f, "call {}(", name)?;
            write_tmp_list(args, f)?;
            write!(f, ")")
        }
        Op::Variant {
            enum_name,
            variant,
            args,
        } => {
            write!(f, "variant {}::{}(", enum_name, variant)?;
            write_tmp_list(args, f)?;
            write!(f, ")")
        }
        Op::EmptyList => write!(f, "empty_list"),
        Op::List(elems) => {
            write!(f, "list [")?;
            write_tmp_list(elems, f)?;
            write!(f, "]")
        }
        Op::ListPush { list, value } => write!(f, "list_push {}, {}", list, value),
        Op::IterInit(tmp) => write!(f, "iter_init {}", tmp),
        Op::Struct { name, fields } => {
            write!(f, "struct {} {{ ", name)?;
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                match field {
                    HirStructField::Set { name, value } => write!(f, "{}: {}", name, value)?,
                    HirStructField::Spread(tmp) => write!(f, "...{}", tmp)?,
                }
            }
            write!(f, " }}")
        }
        Op::Copy(tmp) => write!(f, "copy {}", tmp),
    }
}

fn write_tmp_list(tmps: &[Tmp], f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    for (i, tmp) in tmps.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{}", tmp)?;
    }
    Ok(())
}

impl PrettyPrint for Terminator {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        match self {
            Terminator::Jump(target) => writeln!(f, "jump -> {}", target),
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => writeln!(f, "branch {} -> {}, {}", cond, then_block, else_block),
            Terminator::Return(tmp) => writeln!(f, "return {}", tmp),
            Terminator::IterNext {
                iter,
                value,
                body,
                exit,
            } => writeln!(f, "iter_next {} -> {}, {}, {}", iter, value, body, exit),
        }
    }
}
