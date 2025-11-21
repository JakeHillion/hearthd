pub mod api;
mod config;

#[cfg(doc)]
pub mod examples;

pub use config::{Config, Diagnostic, Diagnostics, LogLevel, format_diagnostics};
