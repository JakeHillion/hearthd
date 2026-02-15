//! Representations for the HearthD Automations language.
//!
//! This module contains the AST and lowered AST types, along with
//! pretty-printing utilities for debugging and testing.

pub mod ast;
pub mod hir;
pub mod lowered;
pub mod pretty_print;
pub mod typed;

// Pretty print impls (use the same PrettyPrint trait)
mod hir_pretty_print;
mod lowered_pretty_print;
mod typed_pretty_print;

// Re-export AST types at the repr level
pub use ast::*;
// Re-export HIR types
pub use hir::{
    BasicBlock, BlockId, HirAutomation, HirBinOp, HirProgram, HirStructField, Instruction, Op,
    Param, Terminator, Tmp,
};
// Re-export lowered AST types with a Lowered prefix already in their names
pub use lowered::{
    LoweredArg, LoweredAutomation, LoweredExpr, LoweredProgram, LoweredStmt, LoweredStructField,
    Origin, Spanned as LoweredSpanned,
};
// Re-export pretty printing
pub use pretty_print::PrettyPrint;
// Re-export typed AST types
pub use typed::{
    CheckResult, EntityConstraint, Ty, TypedArg, TypedAutomation, TypedExpr, TypedExprKind,
    TypedProgram, TypedStmt, TypedStructField,
};
