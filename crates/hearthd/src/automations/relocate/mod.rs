//! Relocation pass: `RelocatableProgram` → `BytecodeProgram`.
//!
//! Compilation stops at a program that names entities without knowing what
//! they are. This pass resolves those names against one deployment, and is
//! the only way to produce the [`Bytecode`] the VM runs.
//!
//! Keeping it separate is what makes a late device cheap. Zigbee2MQTT
//! discovery is not instant, and a house gains and loses devices while it is
//! running; an automation naming one that has not turned up yet fails to
//! relocate, not to compile. When it does turn up, the same compiled program
//! is relocated again against the new index. Nothing is re-parsed, re-typed
//! or re-encoded.
//!
//! Only the constant pool is rewritten. The instruction stream is carried
//! across byte for byte, so what a deployment executes is what the compiler
//! emitted.

mod bytecode;
mod disassemble;
#[allow(clippy::module_inception)]
mod relocate;

#[cfg(test)]
mod tests;

pub use bytecode::Bytecode;
pub use bytecode::BytecodeAutomation;
pub use bytecode::BytecodeProgram;
pub use relocate::Unresolved;
pub use relocate::relocate_program;
