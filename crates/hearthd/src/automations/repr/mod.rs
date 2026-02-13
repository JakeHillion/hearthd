//! Representations for the HearthD Automations language.
//!
//! This module contains the AST and lowered AST types, along with
//! pretty-printing utilities for debugging and testing.

pub mod ast;
pub mod lowered;
pub mod pretty_print;

// Lowered pretty print impls (uses the same PrettyPrint trait)
mod lowered_pretty_print;

// Re-export AST types at the repr level
pub use ast::*;
// Re-export lowered AST types with a Lowered prefix already in their names
pub use lowered::{
    LoweredArg, LoweredAutomation, LoweredExpr, LoweredProgram, LoweredStmt, LoweredStructField,
    Origin, Spanned as LoweredSpanned,
};
// Re-export pretty printing
pub use pretty_print::PrettyPrint;
