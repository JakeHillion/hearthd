mod diagnostics;

pub use hearthd_config_derive::{MergeableConfig, SubConfig};

// Re-export diagnostic types
pub use diagnostics::{
    format_diagnostics, Diagnostic, Diagnostics, Error, LoadError, MergeConflictLocation,
    MergeError, SourceInfo, ValidationError, Warning,
};
