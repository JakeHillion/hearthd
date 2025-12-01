#[allow(clippy::module_inception)]
mod config;

pub use config::*;
// Re-export diagnostics from hearthd_config (the proc-macro based implementation)
pub use hearthd_config::{Diagnostic, Diagnostics, format_diagnostics};
