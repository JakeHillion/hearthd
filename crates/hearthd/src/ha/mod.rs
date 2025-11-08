//! Home Assistant integration support.
//!
//! This module provides support for running Home Assistant integrations
//! in a sandboxed Python environment, communicating with the Rust runtime
//! via Unix domain sockets.

pub mod sandbox;

pub use registry::Registry;
pub use sandbox::Sandbox;
pub use sandbox::SandboxBuilder;

mod integration;
mod protocol;
mod registry;

use integration::Integration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Ser/De error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("invalid message, expected `{expected}`, but got: {received:?}")]
    InvalidMessage {
        expected: String,
        received: protocol::Message,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = ::core::result::Result<T, Error>;
