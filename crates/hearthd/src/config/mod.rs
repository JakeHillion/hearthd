mod config;
mod diagnostics;
mod partial;

pub use config::*;
// Re-export diagnostics from hearthd_config (the new proc-macro based implementation)
pub use hearthd_config::{format_diagnostics, Diagnostic, Diagnostics};

// Re-export specific types for clarity
pub use config::LogLevel;
