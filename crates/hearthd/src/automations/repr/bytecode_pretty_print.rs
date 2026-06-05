//! Disassembler / pretty-printer for [`super::bytecode::Bytecode`].
//!
//! Decodes the byte stream back into a textual form suitable for snapshot
//! tests. Each instruction is shown with its byte offset so jump targets
//! are readable.

use super::ast;
use super::bytecode::*;
use super::function::FunctionIdentity;
use super::hir::HirBinOp;
use super::pretty_print::PrettyPrint;
use super::pretty_print::write_indent;

impl PrettyPrint for BytecodeProgram {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BytecodeProgram::Automation(auto) => auto.pretty_print(indent, f),
            BytecodeProgram::Template {
                params,
                automations,
            } => write_template(params, automations, indent, f),
        }
    }
}

impl PrettyPrint for BytecodeAutomation {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_automation(self.kind, self.filter.as_ref(), &self.body, indent, f)
    }
}

impl PrettyPrint for Bytecode {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let listing: Vec<String> = self.consts.iter().map(verbose).collect();
        let briefs: Vec<String> = self.consts.iter().map(brief).collect();
        write_function(
            &self.params,
            self.num_regs,
            &listing,
            &briefs,
            &self.code,
            indent,
            f,
        )
    }
}

impl PrettyPrint for RelocatableProgram {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelocatableProgram::Automation(auto) => auto.pretty_print(indent, f),
            RelocatableProgram::Template {
                params,
                automations,
            } => write_template(params, automations, indent, f),
        }
    }
}

impl PrettyPrint for RelocatableAutomation {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_automation(self.kind, self.filter.as_ref(), &self.body, indent, f)
    }
}

impl PrettyPrint for RelocatableBytecode {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let listing: Vec<String> = self.consts.iter().map(reloc_verbose).collect();
        let briefs: Vec<String> = self.consts.iter().map(reloc_brief).collect();
        write_function(
            &self.params,
            self.num_regs,
            &listing,
            &briefs,
            &self.code,
            indent,
            f,
        )
    }
}

/// The body every function form prints, over an already-rendered pool.
///
/// Relocation changes what a pool slot holds and nothing else, so a
/// relocatable function and the bytecode it becomes render identically
/// apart from the slots that were symbols.
fn write_function(
    params: &[BytecodeParam],
    num_regs: u32,
    listing: &[String],
    briefs: &[String],
    code: &[u8],
    indent: usize,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    write_indent(indent, f)?;
    writeln!(f, "regs: {}", num_regs)?;
    if !params.is_empty() {
        write_indent(indent, f)?;
        writeln!(f, "params:")?;
        for param in params {
            write_indent(indent + 1, f)?;
            writeln!(f, "r{}: {} [{}]", param.reg, param.name, param.ty)?;
        }
    }
    if !listing.is_empty() {
        write_indent(indent, f)?;
        writeln!(f, "consts:")?;
        for (i, c) in listing.iter().enumerate() {
            write_indent(indent + 1, f)?;
            writeln!(f, "#{} = {}", i, c)?;
        }
    }
    write_indent(indent, f)?;
    writeln!(f, "code:")?;
    disassemble(code, briefs, indent + 1, f)
}

/// The `Template:` header both program forms print.
fn write_template<A: PrettyPrint>(
    params: &[ast::Spanned<ast::TemplateParam>],
    automations: &[A],
    indent: usize,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
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

/// The `Automation:` header both automation forms print.
fn write_automation<B: PrettyPrint>(
    kind: ast::AutomationKind,
    filter: Option<&B>,
    body: &B,
    indent: usize,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    write_indent(indent, f)?;
    writeln!(f, "Automation: {}", kind)?;
    if let Some(filter) = filter {
        write_indent(indent + 1, f)?;
        writeln!(f, "filter:")?;
        filter.pretty_print(indent + 2, f)?;
    }
    write_indent(indent + 1, f)?;
    writeln!(f, "body:")?;
    body.pretty_print(indent + 2, f)
}

/// One resolved pool entry as the `consts:` listing names it, which says
/// what kind of constant it is where the operand form does not.
fn verbose(c: &Const) -> String {
    match c {
        Const::Int(n) => format!("int {}", n),
        Const::Float(n) => format!("float {}", n),
        Const::String(s) => format!("string \"{}\"", s),
        Const::Ident(s) => format!("ident {}", s),
        Const::UnitLit { value, unit } => format!("unit {}{}", value, unit),
        Const::Node(id) => format!("node {}", id),
    }
}

/// One pool entry as the listing names it, relocated or not.
fn reloc_verbose(c: &RelocConst) -> String {
    match c {
        RelocConst::Resolved(konst) => verbose(konst),
        RelocConst::Symbol(symbol) => format!("entity {}", symbol),
    }
}

fn disassemble(
    code: &[u8],
    consts: &[String],
    indent: usize,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    let mut pc = 0;
    while pc < code.len() {
        let start = pc;
        let opcode = Opcode::from_repr(code[pc])
            .unwrap_or_else(|| panic!("unknown opcode 0x{:02x} at offset {}", code[pc], pc));
        pc += 1;
        write_indent(indent, f)?;
        write!(f, "{:04}: ", start)?;
        match opcode {
            Opcode::LoadConstInt
            | Opcode::LoadConstFloat
            | Opcode::LoadConstString
            | Opcode::LoadConstNode => {
                let dst = read_u32(code, &mut pc);
                let idx = read_u32(code, &mut pc);
                let name = match opcode {
                    Opcode::LoadConstInt => "load_const_int",
                    Opcode::LoadConstFloat => "load_const_float",
                    Opcode::LoadConstString => "load_const_string",
                    Opcode::LoadConstNode => "load_const_node",
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
            Opcode::BinOp => {
                let dst = read_u32(code, &mut pc);
                let tag = BinOpTag::from_repr(code[pc]).expect("invalid binop tag");
                pc += 1;
                let lhs = read_u32(code, &mut pc);
                let rhs = read_u32(code, &mut pc);
                writeln!(
                    f,
                    "{:<18} r{}, {}, r{}, r{}",
                    "binop",
                    dst,
                    HirBinOp::from(tag),
                    lhs,
                    rhs
                )?;
            }
            Opcode::Neg | Opcode::Not | Opcode::Deref => {
                let dst = read_u32(code, &mut pc);
                let src = read_u32(code, &mut pc);
                let name = match opcode {
                    Opcode::Neg => "neg",
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
                writeln!(f, "{:<18} {:04}", "jump", target)?;
            }
            Opcode::JumpIf => {
                let cond = read_u32(code, &mut pc);
                let then_t = read_u32(code, &mut pc);
                let else_t = read_u32(code, &mut pc);
                writeln!(
                    f,
                    "{:<18} r{}, {:04}, {:04}",
                    "jump_if", cond, then_t, else_t
                )?;
            }
            Opcode::IterNext => {
                let iter = read_u32(code, &mut pc);
                let value = read_u32(code, &mut pc);
                let body = read_u32(code, &mut pc);
                let exit = read_u32(code, &mut pc);
                writeln!(
                    f,
                    "{:<18} r{}, r{}, {:04}, {:04}",
                    "iter_next", iter, value, body, exit
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

/// The pool entry an operand names, as the disassembly shows it.
fn const_brief(consts: &[String], idx: u32) -> String {
    consts[idx as usize].clone()
}

/// One resolved pool entry in brief form.
fn brief(c: &Const) -> String {
    match c {
        Const::Int(n) => format!("int {}", n),
        Const::Float(n) => format!("float {}", n),
        Const::String(s) => format!("\"{}\"", s),
        Const::Ident(s) => s.clone(),
        Const::UnitLit { value, unit } => format!("{}{}", value, unit),
        Const::Node(id) => format!("node {}", id),
    }
}

/// One pool entry in brief form, whether or not it has been relocated.
fn reloc_brief(c: &RelocConst) -> String {
    match c {
        RelocConst::Resolved(konst) => brief(konst),
        RelocConst::Symbol(symbol) => format!("entity {}", symbol),
    }
}
