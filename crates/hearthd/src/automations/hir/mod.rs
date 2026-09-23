//! The HIR, and the pass that lowers a type-checked program into it.
//!
//! A `HirProgram` is a directed graph of basic blocks. Variable names are
//! gone, replaced by numbered temporaries; entity references remain symbolic
//! for later linking.

#[allow(clippy::module_inception)]
mod hir;
mod lower;
mod pretty_print;

#[cfg(test)]
mod tests;

pub use hir::BasicBlock;
pub use hir::BlockId;
pub use hir::HirAutomation;
pub use hir::HirBinOp;
pub use hir::HirFunction;
pub use hir::HirProgram;
pub use hir::HirStructField;
pub use hir::Instruction;
pub use hir::NumTy;
pub use hir::Op;
pub use hir::Param;
pub use hir::Terminator;
pub use hir::Tmp;
pub use lower::lower_program;
