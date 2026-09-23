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

pub use hir::*;
pub use lower::lower_program;
