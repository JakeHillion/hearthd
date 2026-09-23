//! The bytecode, the pass that encodes LIR into it, and its disassembler.
//!
//! `Bytecode` is the compact, encoded form of a function, ready for the VM.
//! Opcode numbering, operand widths and the constant pool's layout are
//! decided here and nowhere else: the encoder and the disassembler are the
//! only two things that need to agree on them.

#[allow(clippy::module_inception)]
mod bytecode;
mod disassemble;
mod lower;

#[cfg(test)]
mod tests;

pub use bytecode::*;
pub use lower::lower_bytecode_program;
