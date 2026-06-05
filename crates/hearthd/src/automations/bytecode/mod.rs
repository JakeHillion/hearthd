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

pub use bytecode::Bytecode;
pub use bytecode::BytecodeAutomation;
pub use bytecode::BytecodeParam;
pub use bytecode::BytecodeProgram;
pub use bytecode::Const;
pub use bytecode::EntitySymbol;
pub use bytecode::FunctionTag;
pub use bytecode::Opcode;
pub use bytecode::RelocConst;
pub use bytecode::RelocatableAutomation;
pub use bytecode::RelocatableBytecode;
pub use bytecode::RelocatableProgram;
pub use bytecode::StructFieldTag;
pub use lower::lower_bytecode_program;
