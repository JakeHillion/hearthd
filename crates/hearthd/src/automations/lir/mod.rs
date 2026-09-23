//! The LIR, and the pass that lowers HIR basic blocks into it.
//!
//! A `LirFunction` is a flat instruction stream. Block terminators have become
//! ordinary instructions preceded by a `Label`, so a jump resolves to a
//! position in the stream rather than to a block id.

#[allow(clippy::module_inception)]
mod lir;
mod lower;
mod pretty_print;

#[cfg(test)]
mod tests;

pub use lir::Label;
pub use lir::LirAutomation;
pub use lir::LirFunction;
pub use lir::LirInstr;
pub use lir::LirParam;
pub use lir::LirProgram;
pub use lir::LirStructField;
pub use lir::Reg;
pub use lower::lower_lir_program;
