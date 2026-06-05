//! Bytecode VM for the HearthD Automations language.
//!
//! [`Vm`] is a flat register machine holding one compiled function. It takes
//! ownership of that function's code and constant pool at construction, so a
//! `Vm` is free of the `Bytecode` it was built from.
//!
//! A filter's `Vm` is built once and run many times: [`Vm::run_sync`] binds a
//! fresh set of parameters, resets the register file in place, and runs to
//! completion, so the register file, code and constant pool are allocated
//! once. A body's cannot be shared that way — a suspended function owns its
//! program counter and registers until it finishes.
//!
//! `run_sync` rejects the `Await` opcode, since filters must not suspend. The
//! async driver for automation bodies reuses the same dispatch loop and
//! arrives separately; it belongs under this module, because resuming from an
//! `Await` needs the machine's internals.
//!
//! The submodules are private and this is the whole public surface.
//! [`Quantity`] and [`IterState`] are here only because [`Value`] variants
//! carry them.

mod consts;
mod error;
mod machine;
mod ops;
mod quantity;
mod value;

#[cfg(test)]
mod tests;

pub use error::VmError;
pub use machine::Vm;
pub use quantity::Quantity;
pub use value::IterState;
pub use value::Value;
