//! The relocated bytecode: a program with every entity it names resolved.
//!
//! This is what the relocation pass produces and the only form the VM
//! accepts. It differs from a
//! [`RelocatableBytecode`](crate::automations::bytecode::RelocatableBytecode)
//! in its constant pool alone — the instruction stream is carried across
//! byte for byte — so the encoding these types are read with belongs to
//! [`crate::automations::bytecode`], which decides it.
//!
//! Nothing outside [`super`] constructs one. A `Bytecode` asserts that every
//! entity in its pool resolved against some deployment, and relocation is
//! the only thing that can establish that.

use crate::automations::bytecode::BytecodeParam;
use crate::automations::bytecode::Const;
use crate::automations::parser::ast;

/// A single compiled function ready for the VM.
#[derive(Debug, Clone)]
pub struct Bytecode {
    pub params: Vec<BytecodeParam>,
    pub num_regs: u32,
    pub consts: Vec<Const>,
    pub code: Vec<u8>,
}

/// A compiled automation: filter (optional) + body, both as `Bytecode`.
#[derive(Debug, Clone)]
pub struct BytecodeAutomation {
    pub kind: ast::AutomationKind,
    pub filter: Option<Bytecode>,
    pub body: Bytecode,
}

/// A compiled program.
#[derive(Debug, Clone)]
pub enum BytecodeProgram {
    Automation(BytecodeAutomation),
    Template {
        params: Vec<ast::Spanned<ast::TemplateParam>>,
        automations: Vec<BytecodeAutomation>,
    },
}
