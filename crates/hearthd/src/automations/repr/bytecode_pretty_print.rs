//! Disassembler / pretty-printer for [`super::bytecode::Bytecode`].
//!
//! Decodes the byte stream back into a textual form suitable for snapshot
//! tests. Jump operands print as labels rather than byte offsets, so the
//! output describes control flow instead of byte layout and stays stable
//! when instruction encodings change.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use super::bytecode::*;
use super::function::FunctionIdentity;
use super::pretty_print::PrettyPrint;
use super::pretty_print::write_indent;

/// The mnemonic for a binary opcode, which names its operator and — for
/// the numeric ones — the type it is specialised to.
fn binary_name(opcode: Opcode) -> &'static str {
    match opcode {
        Opcode::AddInt => "add_int",
        Opcode::SubInt => "sub_int",
        Opcode::MulInt => "mul_int",
        Opcode::DivInt => "div_int",
        Opcode::ModInt => "mod_int",
        Opcode::LtInt => "lt_int",
        Opcode::LeInt => "le_int",
        Opcode::GtInt => "gt_int",
        Opcode::GeInt => "ge_int",
        Opcode::AddFloat => "add_float",
        Opcode::SubFloat => "sub_float",
        Opcode::MulFloat => "mul_float",
        Opcode::DivFloat => "div_float",
        Opcode::ModFloat => "mod_float",
        Opcode::LtFloat => "lt_float",
        Opcode::LeFloat => "le_float",
        Opcode::GtFloat => "gt_float",
        Opcode::GeFloat => "ge_float",
        Opcode::Eq => "eq",
        Opcode::Ne => "ne",
        Opcode::In => "in",
        other => unreachable!("{:?} is not a binary opcode", other),
    }
}

impl PrettyPrint for BytecodeProgram {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BytecodeProgram::Automation(auto) => auto.pretty_print(indent, f),
            BytecodeProgram::Template {
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

impl PrettyPrint for BytecodeAutomation {
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

impl PrettyPrint for Bytecode {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indent(indent, f)?;
        writeln!(f, "regs: {}", self.num_regs)?;
        if !self.params.is_empty() {
            write_indent(indent, f)?;
            writeln!(f, "params:")?;
            for param in &self.params {
                write_indent(indent + 1, f)?;
                writeln!(f, "r{}: {} [{}]", param.reg, param.name, param.ty)?;
            }
        }
        if !self.consts.is_empty() {
            write_indent(indent, f)?;
            writeln!(f, "consts:")?;
            for (i, c) in self.consts.iter().enumerate() {
                write_indent(indent + 1, f)?;
                write!(f, "#{} = ", i)?;
                write_const(c, f)?;
                writeln!(f)?;
            }
        }
        write_indent(indent, f)?;
        writeln!(f, "code:")?;
        disassemble(&self.code, &self.consts, indent + 1, f)
    }
}

fn write_const(c: &Const, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match c {
        Const::Int(n) => write!(f, "int {}", n),
        Const::Float(n) => write!(f, "float {}", n),
        Const::String(s) => write!(f, "string \"{}\"", s),
        Const::Ident(s) => write!(f, "ident {}", s),
        Const::UnitLit { value, unit } => write!(f, "unit {}{}", value, unit),
    }
}

/// The offsets that are jumped to, named `l0`, `l1`, ... in ascending order.
#[derive(Default)]
struct Labels {
    names: BTreeMap<u32, usize>,
}

/// Instruction boundaries and jump targets recorded while decoding.
#[derive(Default)]
struct Scan {
    starts: BTreeSet<u32>,
    targets: BTreeSet<u32>,
}

impl Labels {
    /// Decode the stream once to find which offsets are jumped to. The
    /// disassembly this produces goes to a scratch buffer and is discarded;
    /// only the recorded boundaries and targets are kept. Decoding through
    /// the printer keeps a single description of the instruction layout.
    fn scan(code: &[u8], consts: &[Const]) -> Labels {
        let mut scan = Scan::default();
        let mut scratch = String::new();
        write_instructions(code, consts, &Labels::default(), 0, &mut scratch, &mut scan)
            .expect("writing to a String cannot fail");
        Labels {
            names: scan
                .targets
                .iter()
                .filter(|target| scan.starts.contains(target))
                .enumerate()
                .map(|(i, &target)| (target, i))
                .collect(),
        }
    }

    /// Render a jump operand. A target that is not an instruction boundary
    /// cannot be labelled and is a lowering bug, so say so rather than print
    /// an offset that reads like a normal operand.
    fn target(&self, offset: u32) -> String {
        match self.names.get(&offset) {
            Some(i) => format!("l{}", i),
            None => format!("<invalid target {:04}>", offset),
        }
    }
}

fn disassemble(
    code: &[u8],
    consts: &[Const],
    indent: usize,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    let labels = Labels::scan(code, consts);
    write_instructions(code, consts, &labels, indent, f, &mut Scan::default())
}

fn write_instructions<W: std::fmt::Write>(
    code: &[u8],
    consts: &[Const],
    labels: &Labels,
    indent: usize,
    f: &mut W,
    scan: &mut Scan,
) -> std::fmt::Result {
    let mut pc = 0;
    while pc < code.len() {
        let start = pc;
        scan.starts.insert(start as u32);
        let opcode = Opcode::from_repr(code[pc])
            .unwrap_or_else(|| panic!("unknown opcode 0x{:02x} at offset {}", code[pc], pc));
        pc += 1;
        if let Some(label) = labels.names.get(&(start as u32)) {
            write_indent(indent.saturating_sub(1), f)?;
            writeln!(f, "l{}:", label)?;
        }
        write_indent(indent, f)?;
        match opcode {
            Opcode::LoadConstInt | Opcode::LoadConstFloat | Opcode::LoadConstString => {
                let dst = read_u32(code, &mut pc);
                let idx = read_u32(code, &mut pc);
                let name = match opcode {
                    Opcode::LoadConstInt => "load_const_int",
                    Opcode::LoadConstFloat => "load_const_float",
                    Opcode::LoadConstString => "load_const_string",
                    _ => unreachable!(),
                };
                writeln!(
                    f,
                    "{:<18} r{}, #{} ({})",
                    name,
                    dst,
                    idx,
                    const_brief(consts, idx)
                )?;
            }
            Opcode::LoadConstBool => {
                let dst = read_u32(code, &mut pc);
                let value = code[pc];
                pc += 1;
                writeln!(
                    f,
                    "{:<18} r{}, {}",
                    "load_const_bool",
                    dst,
                    if value != 0 { "true" } else { "false" }
                )?;
            }
            Opcode::LoadConstUnit => {
                let dst = read_u32(code, &mut pc);
                let idx = read_u32(code, &mut pc);
                writeln!(
                    f,
                    "{:<18} r{}, #{} ({})",
                    "load_const_unit",
                    dst,
                    idx,
                    const_brief(consts, idx)
                )?;
            }
            Opcode::Unit => {
                let dst = read_u32(code, &mut pc);
                writeln!(f, "{:<18} r{}", "unit", dst)?;
            }
            Opcode::AddInt
            | Opcode::SubInt
            | Opcode::MulInt
            | Opcode::DivInt
            | Opcode::ModInt
            | Opcode::LtInt
            | Opcode::LeInt
            | Opcode::GtInt
            | Opcode::GeInt
            | Opcode::AddFloat
            | Opcode::SubFloat
            | Opcode::MulFloat
            | Opcode::DivFloat
            | Opcode::ModFloat
            | Opcode::LtFloat
            | Opcode::LeFloat
            | Opcode::GtFloat
            | Opcode::GeFloat
            | Opcode::Eq
            | Opcode::Ne
            | Opcode::In => {
                let dst = read_u32(code, &mut pc);
                let lhs = read_u32(code, &mut pc);
                let rhs = read_u32(code, &mut pc);
                writeln!(
                    f,
                    "{:<18} r{}, r{}, r{}",
                    binary_name(opcode),
                    dst,
                    lhs,
                    rhs
                )?;
            }
            Opcode::Neg | Opcode::ToFloat | Opcode::Not | Opcode::Deref => {
                let dst = read_u32(code, &mut pc);
                let src = read_u32(code, &mut pc);
                let name = match opcode {
                    Opcode::Neg => "neg",
                    Opcode::ToFloat => "to_float",
                    Opcode::Not => "not",
                    Opcode::Deref => "deref",
                    _ => unreachable!(),
                };
                writeln!(f, "{:<18} r{}, r{}", name, dst, src)?;
            }
            Opcode::Field | Opcode::OptionalField => {
                let dst = read_u32(code, &mut pc);
                let base = read_u32(code, &mut pc);
                let idx = read_u32(code, &mut pc);
                let name = if matches!(opcode, Opcode::Field) {
                    "field"
                } else {
                    "optional_field"
                };
                writeln!(
                    f,
                    "{:<18} r{}, r{}, #{} ({})",
                    name,
                    dst,
                    base,
                    idx,
                    const_brief(consts, idx)
                )?;
            }
            Opcode::Call => {
                let dst = read_u32(code, &mut pc);
                let tag = FunctionTag::from_repr(code[pc]).expect("invalid function tag");
                pc += 1;
                let n = read_u32(code, &mut pc);
                let args: Vec<u32> = (0..n).map(|_| read_u32(code, &mut pc)).collect();
                writeln!(
                    f,
                    "{:<18} r{}, {}, [{}]",
                    "call",
                    dst,
                    FunctionIdentity::from(tag),
                    args.iter()
                        .map(|r| format!("r{}", r))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
            Opcode::Variant => {
                let dst = read_u32(code, &mut pc);
                let enum_idx = read_u32(code, &mut pc);
                let variant_idx = read_u32(code, &mut pc);
                let n = read_u32(code, &mut pc);
                let args: Vec<u32> = (0..n).map(|_| read_u32(code, &mut pc)).collect();
                writeln!(
                    f,
                    "{:<18} r{}, #{} ({}), #{} ({}), [{}]",
                    "variant",
                    dst,
                    enum_idx,
                    const_brief(consts, enum_idx),
                    variant_idx,
                    const_brief(consts, variant_idx),
                    args.iter()
                        .map(|r| format!("r{}", r))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
            Opcode::EmptyList => {
                let dst = read_u32(code, &mut pc);
                writeln!(f, "{:<18} r{}", "empty_list", dst)?;
            }
            Opcode::List => {
                let dst = read_u32(code, &mut pc);
                let n = read_u32(code, &mut pc);
                let elems: Vec<u32> = (0..n).map(|_| read_u32(code, &mut pc)).collect();
                writeln!(
                    f,
                    "{:<18} r{}, [{}]",
                    "list",
                    dst,
                    elems
                        .iter()
                        .map(|r| format!("r{}", r))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
            Opcode::ListPush => {
                let list = read_u32(code, &mut pc);
                let value = read_u32(code, &mut pc);
                writeln!(f, "{:<18} r{}, r{}", "list_push", list, value)?;
            }
            Opcode::IterInit => {
                let dst = read_u32(code, &mut pc);
                let src = read_u32(code, &mut pc);
                writeln!(f, "{:<18} r{}, r{}", "iter_init", dst, src)?;
            }
            Opcode::Struct => {
                let dst = read_u32(code, &mut pc);
                let name_idx = read_u32(code, &mut pc);
                let n = read_u32(code, &mut pc);
                let mut entries = Vec::new();
                for _ in 0..n {
                    let tag = StructFieldTag::from_repr(code[pc])
                        .unwrap_or_else(|| panic!("invalid struct field tag {}", code[pc]));
                    pc += 1;
                    match tag {
                        StructFieldTag::Set => {
                            let field_idx = read_u32(code, &mut pc);
                            let value = read_u32(code, &mut pc);
                            entries.push(format!("{}: r{}", const_brief(consts, field_idx), value));
                        }
                        StructFieldTag::Spread => {
                            let src = read_u32(code, &mut pc);
                            entries.push(format!("...r{}", src));
                        }
                    }
                }
                writeln!(
                    f,
                    "{:<18} r{}, #{} ({}), {{ {} }}",
                    "struct",
                    dst,
                    name_idx,
                    const_brief(consts, name_idx),
                    entries.join(", ")
                )?;
            }
            Opcode::Copy => {
                let dst = read_u32(code, &mut pc);
                let src = read_u32(code, &mut pc);
                writeln!(f, "{:<18} r{}, r{}", "copy", dst, src)?;
            }
            Opcode::Jump => {
                let target = read_u32(code, &mut pc);
                scan.targets.insert(target);
                writeln!(f, "{:<18} {}", "jump", labels.target(target))?;
            }
            Opcode::JumpIf => {
                let cond = read_u32(code, &mut pc);
                let then_t = read_u32(code, &mut pc);
                let else_t = read_u32(code, &mut pc);
                scan.targets.insert(then_t);
                scan.targets.insert(else_t);
                writeln!(
                    f,
                    "{:<18} r{}, {}, {}",
                    "jump_if",
                    cond,
                    labels.target(then_t),
                    labels.target(else_t)
                )?;
            }
            Opcode::IterNext => {
                let iter = read_u32(code, &mut pc);
                let value = read_u32(code, &mut pc);
                let body = read_u32(code, &mut pc);
                let exit = read_u32(code, &mut pc);
                scan.targets.insert(body);
                scan.targets.insert(exit);
                writeln!(
                    f,
                    "{:<18} r{}, r{}, {}, {}",
                    "iter_next",
                    iter,
                    value,
                    labels.target(body),
                    labels.target(exit)
                )?;
            }
            Opcode::Return => {
                let src = read_u32(code, &mut pc);
                writeln!(f, "{:<18} r{}", "return", src)?;
            }
            Opcode::Await => {
                let dst = read_u32(code, &mut pc);
                let src = read_u32(code, &mut pc);
                writeln!(f, "{:<18} r{}, r{}", "await", dst, src)?;
            }
        }
    }
    Ok(())
}

fn read_u32(code: &[u8], pc: &mut usize) -> u32 {
    let bytes: [u8; 4] = code[*pc..*pc + 4].try_into().expect("short read");
    *pc += 4;
    u32::from_le_bytes(bytes)
}

fn const_brief(consts: &[Const], idx: u32) -> String {
    match &consts[idx as usize] {
        Const::Int(n) => format!("int {}", n),
        Const::Float(n) => format!("float {}", n),
        Const::String(s) => format!("\"{}\"", s),
        Const::Ident(s) => s.clone(),
        Const::UnitLit { value, unit } => format!("{}{}", value, unit),
    }
}
