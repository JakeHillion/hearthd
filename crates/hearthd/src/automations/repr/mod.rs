//! Representations for the HearthD Automations language.
//!
//! What is left here are the representations below the checker — HIR, LIR and
//! bytecode — along with their pretty printers. Each is being moved to the
//! pass that produces it, as the AST, lowered AST and typed AST already have.

pub mod bytecode;
pub mod hir;
pub mod lir;

// Pretty print impls (use the same PrettyPrint trait)
mod bytecode_pretty_print;
mod hir_pretty_print;
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
// Re-export HIR types
pub use hir::{
    BasicBlock, BlockId, HirAutomation, HirBinOp, HirFunction, HirProgram, HirStructField,
    Instruction, NumTy, Op, Param, Terminator, Tmp,
};
// Re-export LIR types
pub use lir::{
    Label, LirAutomation, LirFunction, LirInstr, LirParam, LirProgram, LirStructField, Reg,
};
