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
//! `run_sync` rejects the `Await` opcode, since filters must not suspend.
//! [`Vm::run_async`] is the driver for bodies. It shares the dispatch loop and
//! differs only at the `Await`, where it decodes the operands the loop leaves
//! undecoded and hands the suspension to a [`Suspension`]. It lives here
//! rather than beside the runner because resuming from an `Await` needs the
//! machine's internals, and it takes the `Vm` by value because a suspended
//! function cannot also be a shared template.
//!
//! What a suspension evaluates to is the caller's to decide, not the
//! machine's: `sleep_unique` fails when a newer instance of the same
//! automation exists, and a register machine has no notion of an instance.
//! [`Timer`] is the answer for a driver with nothing to supersede it.
//!
//! A `Vm` holds its program behind an `Arc`, so [`Vm::instance`] gives a
//! concurrent run its own register file over one shared instruction stream
//! and constant pool. That is what lets several instances of one body be
//! alive at once without several copies of it.
//!
//! The submodules are private and this is the whole public surface.
//! [`Quantity`], [`IterState`] and [`Pending`] are here only because
//! [`Value`] variants carry them.

mod consts;
mod error;
mod machine;
mod ops;
mod quantity;
mod suspension;
mod value;

#[cfg(test)]
mod tests;

pub use error::VmError;
pub use machine::Vm;
pub use quantity::Quantity;
pub use suspension::Suspension;
pub use suspension::Timer;
pub use value::IterState;
pub use value::Pending;
pub use value::Value;
