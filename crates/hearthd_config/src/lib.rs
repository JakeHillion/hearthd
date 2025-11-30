mod diagnostics;
mod located;

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
pub use located::Located;

/// Trait that associates a config type with its partial variant.
///
/// This trait is automatically implemented by the `MergeableConfig` and `SubConfig`
/// derive macros. It allows the macro-generated code to reference the partial type
/// without requiring it to be directly in scope.
pub trait HasPartialConfig {
    /// The partial configuration type (where all fields are optional and wrapped in Spanned)
    type PartialConfig;
}
