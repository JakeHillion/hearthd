//! Pretty-printing for LIR programs.
//!
//! Used by codegen tests to produce readable snapshot output.

use super::lir::*;
use super::pretty_print::PrettyPrint;

impl std::fmt::Display for ConstIdx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "c{}", self.0)
    }
}

impl std::fmt::Display for SymIdx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s{}", self.0)
    }
}

impl std::fmt::Display for Reg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}", self.0)
    }
}

impl std::fmt::Display for Constant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Constant::Int(n) => write!(f, "int({})", n),
            Constant::Float(n) => write!(f, "float({})", n),
            Constant::String(s) => write!(f, "string(\"{}\")", s),
            Constant::Bool(b) => write!(f, "bool({})", b),
            Constant::Unit { value, unit } => write!(f, "unit({}{})", value, unit),
            Constant::Void => write!(f, "void"),
        }
    }
}

impl PrettyPrint for LirProgram {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Constants
        write_indent(indent, f)?;
        writeln!(f, "Constants:")?;
        if self.constant_pool.is_empty() {
            write_indent(indent + 1, f)?;
            writeln!(f, "(none)")?;
        } else {
            for (i, c) in self.constant_pool.iter().enumerate() {
                write_indent(indent + 1, f)?;
                writeln!(f, "c{} = {}", i, c)?;
            }
        }

        // Symbols
        write_indent(indent, f)?;
        writeln!(f, "Symbols:")?;
        if self.symbol_table.is_empty() {
            write_indent(indent + 1, f)?;
            writeln!(f, "(none)")?;
        } else {
            for (i, s) in self.symbol_table.iter().enumerate() {
                write_indent(indent + 1, f)?;
                writeln!(f, "s{} = \"{}\"", i, s.0)?;
            }
        }

        // Automations
        for auto in &self.automations {
            auto.pretty_print(indent, f)?;
        }
        Ok(())
    }
}

impl PrettyPrint for LirAutomation {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        writeln!(f, "Automation: {}", self.kind)?;

        if !self.params.is_empty() {
            write_indent(indent + 1, f)?;
            writeln!(f, "Params:")?;
            for param in &self.params {
                write_indent(indent + 2, f)?;
                writeln!(f, "{}: {}", param.reg, param.name)?;
            }
        }

        write_indent(indent + 1, f)?;
        writeln!(f, "Registers: {}", self.register_count)?;

        write_indent(indent + 1, f)?;
        writeln!(f, "Instructions:")?;
        for (i, instr) in self.instructions.iter().enumerate() {
            write_indent(indent + 2, f)?;
            write!(f, "{:04}: ", i)?;
            write_instruction(instr, f)?;
            writeln!(f)?;
        }
        Ok(())
    }
}

fn write_indent(indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    for _ in 0..indent {
        write!(f, "  ")?;
    }
    Ok(())
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

fn write_instruction(instr: &LirInstruction, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match instr {
        LirInstruction::LoadConst { dst, idx } => write!(f, "{} = load_const {}", dst, idx),
        LirInstruction::BinOp {
            dst,
            op,
            left,
            right,
        } => write!(f, "{} = {} {}, {}", dst, op, left, right),
        LirInstruction::Neg { dst, src } => write!(f, "{} = neg {}", dst, src),
        LirInstruction::Not { dst, src } => write!(f, "{} = not {}", dst, src),
        LirInstruction::Deref { dst, src } => write!(f, "{} = deref {}", dst, src),
        LirInstruction::Await { dst, src } => write!(f, "{} = await {}", dst, src),
        LirInstruction::Field { dst, base, field } => {
            write!(f, "{} = field {}.{}", dst, base, field)
        }
        LirInstruction::OptionalField { dst, base, field } => {
            write!(f, "{} = optional_field {}?.{}", dst, base, field)
        }
        LirInstruction::Call { dst, func, args } => {
            write!(f, "{} = call {}(", dst, func)?;
            write_reg_list(args, f)?;
            write!(f, ")")
        }
        LirInstruction::Variant {
            dst,
            enum_name,
            variant,
            args,
        } => {
            write!(f, "{} = variant {}::{}(", dst, enum_name, variant)?;
            write_reg_list(args, f)?;
            write!(f, ")")
        }
        LirInstruction::EmptyList { dst } => write!(f, "{} = empty_list", dst),
        LirInstruction::List { dst, elements } => {
            write!(f, "{} = list [", dst)?;
            write_reg_list(elements, f)?;
            write!(f, "]")
        }
        LirInstruction::ListPush { dst, list, value } => {
            write!(f, "{} = list_push {}, {}", dst, list, value)
        }
        LirInstruction::IterInit { dst, src } => write!(f, "{} = iter_init {}", dst, src),
        LirInstruction::Struct { dst, name, fields } => {
            write!(f, "{} = struct {} {{ ", dst, name)?;
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                match field {
                    LirStructField::Set { name, value } => write!(f, "{}: {}", name, value)?,
                    LirStructField::Spread(reg) => write!(f, "...{}", reg)?,
                }
            }
            write!(f, " }}")
        }
        LirInstruction::Copy { dst, src } => write!(f, "{} = copy {}", dst, src),
        LirInstruction::Jump { target } => write!(f, "jump -> {}", target),
        LirInstruction::Branch {
            cond,
            then_target,
            else_target,
        } => write!(f, "branch {} -> {}, {}", cond, then_target, else_target),
        LirInstruction::Return { src } => write!(f, "return {}", src),
        LirInstruction::IterNext {
            iter,
            value,
            body,
            exit,
        } => write!(f, "iter_next {} -> {}, {}, {}", iter, value, body, exit),
    }
}
