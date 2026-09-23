//! Representations for the HearthD Automations language.
//!
//! What is left here are the LIR and the bytecode, along with their pretty
//! printers. Each is being moved to the pass that produces it, as the AST,
//! lowered AST, typed AST and HIR already have.

pub mod bytecode;
pub mod lir;

// Pretty print impls (use the same PrettyPrint trait)
mod bytecode_pretty_print;
mod lir_pretty_print;

// Re-export bytecode types
pub use bytecode::Bytecode;
pub use bytecode::BytecodeAutomation;
pub use bytecode::BytecodeParam;
pub use bytecode::BytecodeProgram;
pub use bytecode::Const;
pub use bytecode::FunctionTag;
pub use bytecode::Opcode;
pub use bytecode::StructFieldTag;
// Re-export LIR types
pub use lir::{
    Label, LirAutomation, LirFunction, LirInstr, LirParam, LirProgram, LirStructField, Reg,
};
