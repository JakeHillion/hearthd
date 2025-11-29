mod config;

// Re-export specific types for clarity
pub use config::LogLevel;
pub use config::*;
// Re-export diagnostics from hearthd_config (the proc-macro based implementation)
pub use hearthd_config::{Diagnostic, Diagnostics, format_diagnostics};
