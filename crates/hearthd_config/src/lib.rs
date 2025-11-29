mod diagnostics;

// Re-export diagnostic types
pub use diagnostics::Diagnostic;
pub use diagnostics::Diagnostics;
pub use diagnostics::Error;
pub use diagnostics::LoadError;
pub use diagnostics::MergeConflictLocation;
pub use diagnostics::MergeError;
pub use diagnostics::SourceInfo;
pub use diagnostics::ValidationError;
pub use diagnostics::Warning;
pub use diagnostics::format_diagnostics;
pub use hearthd_config_derive::MergeableConfig;
pub use hearthd_config_derive::SubConfig;
