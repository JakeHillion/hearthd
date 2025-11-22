pub mod api;
mod config;
mod engine;

#[cfg(feature = "integration_mqtt")]
mod integrations;

#[cfg(doc)]
pub mod examples;

pub use config::{Config, LogLevel};
pub use engine::Engine;

// Re-export diagnostic types from internal modules for public API
pub use config::diagnostics::{Diagnostic, Diagnostics, format_diagnostics};
