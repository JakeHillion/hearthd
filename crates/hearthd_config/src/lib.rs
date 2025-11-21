mod diagnostics;

pub use hearthd_config_derive::{MergeableConfig, SubConfig};

// Re-export diagnostic types
pub use diagnostics::{
    Diagnostic, Diagnostics, Error, LoadError, MergeConflictLocation, MergeError, SourceInfo,
    ValidationError, Warning, format_diagnostics,
};
