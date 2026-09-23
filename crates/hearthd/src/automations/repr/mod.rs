//! Representations for the HearthD Automations language.
//!
//! What is left here is the bytecode and its disassembler. Both are being
//! moved to the pass that produces them, as the AST, lowered AST, typed AST,
//! HIR and LIR already have.

pub mod bytecode;

// Pretty print impls (use the same PrettyPrint trait)
mod bytecode_pretty_print;

// Re-export bytecode types
pub use bytecode::Bytecode;
pub use bytecode::BytecodeAutomation;
pub use bytecode::BytecodeParam;
pub use bytecode::BytecodeProgram;
pub use bytecode::Const;
pub use bytecode::FunctionTag;
pub use bytecode::Opcode;
pub use bytecode::StructFieldTag;
