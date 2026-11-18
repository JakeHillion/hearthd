//! The bytecode, the pass that encodes LIR into it, and its disassembler.
//!
//! A `RelocatableBytecode` is the compact, encoded form of a function. It is
//! ready for the VM once [`crate::automations::relocate`] has resolved the
//! entities its constant pool names.
//!
//! Opcode numbering, operand widths and the constant pool's layout are
//! decided here and nowhere else: the encoder and the disassembler are the
//! only two things that need to agree on them. That is why the relocated
//! form's printer lives in [`crate::automations::relocate`] but renders only
//! its constant pool, deferring the rest to the helpers re-exported below.

#[allow(clippy::module_inception)]
mod bytecode;
mod disassemble;
mod lower;

#[cfg(test)]
mod tests;

pub use bytecode::BytecodeParam;
pub use bytecode::Const;
pub use bytecode::EntitySymbol;
pub use bytecode::FunctionTag;
pub use bytecode::Opcode;
pub use bytecode::RelocConst;
pub use bytecode::RelocatableAutomation;
pub use bytecode::RelocatableBytecode;
pub use bytecode::RelocatableProgram;
pub use bytecode::StructFieldTag;
pub use disassemble::brief;
pub use disassemble::verbose;
pub use disassemble::write_automation;
pub use disassemble::write_function;
pub use disassemble::write_template;
pub use lower::lower_bytecode_program;
