//! Home Assistant integration support.
//!
//! This module provides support for running Home Assistant integrations
//! in a sandboxed Python environment, communicating with the Rust runtime
//! via Unix domain sockets.

pub mod protocol;
pub mod runtime;
pub mod sandbox;

pub use runtime::Runtime;
pub use sandbox::Sandbox;
