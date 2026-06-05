//! Pretty-printing for LIR.
//!
//! Used by `lower_lir` snapshot tests to produce readable output.

use super::lir::*;
use super::pretty_print::PrettyPrint;
use super::pretty_print::write_indent;

impl PrettyPrint for LirProgram {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LirProgram::Automation(auto) => auto.pretty_print(indent, f),
            LirProgram::Template {
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

impl PrettyPrint for LirAutomation {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        writeln!(f, "Automation: {}", self.kind)?;
        if let Some(filter) = &self.filter {
            write_indent(indent + 1, f)?;
            writeln!(f, "filter:")?;
            filter.pretty_print(indent + 2, f)?;
        }
        write_indent(indent + 1, f)?;
        writeln!(f, "body:")?;
        self.body.pretty_print(indent + 2, f)
    }
}

impl PrettyPrint for LirFunction {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        writeln!(f, "regs: {}", self.num_regs)?;
        if !self.params.is_empty() {
            write_indent(indent, f)?;
            writeln!(f, "params:")?;
            for param in &self.params {
                write_indent(indent + 1, f)?;
                writeln!(f, "{}: {} [{}]", param.reg, param.name, param.ty)?;
            }
        }
        for instr in &self.instrs {
            instr.pretty_print(indent, f)?;
        }
        Ok(())
    }
}

impl PrettyPrint for LirInstr {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LirInstr::Label(lbl) => {
                // Labels sit one indent level out from instructions for
                // visual grouping.
                if indent > 0 {
                    write_indent(indent - 1, f)?;
                }
                writeln!(f, "{}:", lbl)
            }
            other => {
                write_indent(indent, f)?;
                write_instr(other, f)?;
                writeln!(f)
            }
        }
    }
}

fn write_instr(instr: &LirInstr, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match instr {
        LirInstr::Label(_) => unreachable!("handled in pretty_print"),
        LirInstr::ConstInt { dst, value } => write!(f, "{} = const_int {}", dst, value),
        LirInstr::ConstFloat { dst, value } => write!(f, "{} = const_float {}", dst, value),
        LirInstr::ConstString { dst, value } => write!(f, "{} = const_string \"{}\"", dst, value),
        LirInstr::ConstBool { dst, value } => write!(f, "{} = const_bool {}", dst, value),
        LirInstr::ConstUnit { dst, value, unit } => {
            write!(f, "{} = const_unit {}{}", dst, value, unit)
        }
        LirInstr::Unit { dst } => write!(f, "{} = unit", dst),
        LirInstr::BinOp { dst, op, lhs, rhs } => write!(f, "{} = {} {}, {}", dst, op, lhs, rhs),
        LirInstr::Neg { dst, src } => write!(f, "{} = neg {}", dst, src),
        LirInstr::Not { dst, src } => write!(f, "{} = not {}", dst, src),
        LirInstr::Deref { dst, src } => write!(f, "{} = deref {}", dst, src),
        LirInstr::Field { dst, base, field } => write!(f, "{} = field {}.{}", dst, base, field),
        LirInstr::OptionalField { dst, base, field } => {
            write!(f, "{} = optional_field {}?.{}", dst, base, field)
        }
        LirInstr::Call { dst, name, args } => {
            write!(f, "{} = call {}(", dst, name)?;
            write_reg_list(args, f)?;
            write!(f, ")")
        }
        LirInstr::Variant {
            dst,
            enum_name,
            variant,
            args,
        } => {
            write!(f, "{} = variant {}::{}(", dst, enum_name, variant)?;
            write_reg_list(args, f)?;
            write!(f, ")")
        }
        LirInstr::EmptyList { dst } => write!(f, "{} = empty_list", dst),
        LirInstr::List { dst, elems } => {
            write!(f, "{} = list [", dst)?;
            write_reg_list(elems, f)?;
            write!(f, "]")
        }
        LirInstr::ListPush { list, value } => write!(f, "list_push {}, {}", list, value),
        LirInstr::IterInit { dst, src } => write!(f, "{} = iter_init {}", dst, src),
        LirInstr::Struct { dst, name, fields } => {
            write!(f, "{} = struct {} {{ ", dst, name)?;
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                match field {
                    LirStructField::Set { name, value } => write!(f, "{}: {}", name, value)?,
                    LirStructField::Spread(r) => write!(f, "...{}", r)?,
                }
            }
            write!(f, " }}")
        }
        LirInstr::Copy { dst, src } => write!(f, "{} = copy {}", dst, src),
        LirInstr::Jump(lbl) => write!(f, "jump {}", lbl),
        LirInstr::JumpIf {
            cond,
            then_lbl,
            else_lbl,
        } => write!(f, "jump_if {} -> {}, {}", cond, then_lbl, else_lbl),
        LirInstr::IterNext {
            iter,
            value,
            body_lbl,
            exit_lbl,
        } => write!(
            f,
            "iter_next {} -> {}, {}, {}",
            iter, value, body_lbl, exit_lbl
        ),
        LirInstr::Return(r) => write!(f, "return {}", r),
        LirInstr::Await { dst, src } => write!(f, "{} = await {}", dst, src),
    }
}

fn write_reg_list(regs: &[Reg], f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    for (i, reg) in regs.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{}", reg)?;
    }
    Ok(())
}
