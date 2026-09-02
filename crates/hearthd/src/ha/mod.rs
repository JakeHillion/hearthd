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
mod weather;

#[cfg(all(test, feature = "integration_metno"))]
mod live_tests;

use integration::Integration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Ser/De error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// The payload is boxed because it dwarfs every other variant, and this
    /// error is the `Err` of a `Result` returned throughout the module.
    #[error("invalid message, expected `{expected}`, but got: {received:?}")]
    InvalidMessage {
        expected: String,
        received: Box<protocol::Message>,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Integration setup failed: {name}: {error}")]
    SetupFailed {
        name: String,
        error: String,
        error_type: Option<String>,
        missing_package: Option<String>,
    },
}

pub type Result<T> = ::core::result::Result<T, Error>;
