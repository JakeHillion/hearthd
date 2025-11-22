pub mod api;
mod config;

#[cfg(doc)]
pub mod examples;

pub use config::Config;
pub use config::Diagnostic;
pub use config::Diagnostics;
pub use config::LogLevel;
pub use config::format_diagnostics;
