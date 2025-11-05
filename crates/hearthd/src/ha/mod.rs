//! Home Assistant integration support.
//!
//! This module provides support for running Home Assistant integrations
//! in a sandboxed Python environment, communicating with the Rust runtime
//! via Unix domain sockets.

pub mod protocol;
pub mod sandbox;
pub mod runtime;

pub use protocol::Message;
pub use sandbox::Sandbox;
pub use runtime::Runtime;
