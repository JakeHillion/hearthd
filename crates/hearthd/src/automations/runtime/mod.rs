//! Runtime for executing compiled automations.
//!
//! Today this module provides only the synchronous bytecode VM and its
//! `Value` representation. Async execution (await / sleep), the
//! event-driven `AutomationRunner`, and integration with the engine
//! arrive in subsequent commits.

pub mod runner;
pub mod value;
pub mod vm;

pub use runner::ActionSink;
pub use runner::AutomationId;
pub use runner::CompiledAutomation;
pub use runner::Runner;
pub use value::Value;
pub use vm::VmError;
pub use vm::run_async;
pub use vm::run_sync;
