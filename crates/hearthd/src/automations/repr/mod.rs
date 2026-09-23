//! Representations for the HearthD Automations language.
//!
//! This module contains the AST and lowered AST types, along with
//! pretty-printing utilities for debugging and testing.

pub mod bytecode;
pub mod function;
pub mod hir;
pub mod lir;
pub mod lowered;
pub mod typed;

// Pretty print impls (use the same PrettyPrint trait)
mod bytecode_pretty_print;
mod hir_pretty_print;
mod lir_pretty_print;
mod lowered_pretty_print;
mod typed_pretty_print;

// Re-export bytecode types
pub use bytecode::Bytecode;
pub use bytecode::BytecodeAutomation;
pub use bytecode::BytecodeParam;
pub use bytecode::BytecodeProgram;
pub use bytecode::Const;
pub use bytecode::FunctionTag;
pub use bytecode::Opcode;
pub use bytecode::StructFieldTag;
// Re-export function identities
pub use function::FunctionIdentity;
// Re-export HIR types
pub use hir::{
    BasicBlock, BlockId, HirAutomation, HirBinOp, HirFunction, HirProgram, HirStructField,
    Instruction, NumTy, Op, Param, Terminator, Tmp,
};
// Re-export LIR types
pub use lir::{
    Label, LirAutomation, LirFunction, LirInstr, LirParam, LirProgram, LirStructField, Reg,
};
// Re-export lowered AST types with a Lowered prefix already in their names
pub use lowered::{
    LoweredArg, LoweredAutomation, LoweredExpr, LoweredProgram, LoweredStmt, LoweredStructField,
    Origin, Spanned as LoweredSpanned,
};
// Re-export typed AST types
pub use typed::{
    CheckResult, EntityConstraint, Ty, TypedArg, TypedAutomation, TypedExpr, TypedExprKind,
    TypedProgram, TypedStmt, TypedStructField,
};
