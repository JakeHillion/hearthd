mod diagnostics;
mod document;
mod finalize;
mod from_config;
mod located;
mod scalar;

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
pub use diagnostics::format_diagnostics_plain;
pub use document::Document;
pub use document::Node;
pub use document::Sources;
pub use finalize::TryFromPartial;
pub use from_config::Context;
pub use from_config::Definitions;
pub use from_config::FromConfig;
pub use from_config::Tables;
pub use from_config::from_sources;
pub use from_config::load;
pub use hearthd_config_derive::FromConfig;
pub use hearthd_config_derive::MergeableConfig;
pub use hearthd_config_derive::SubConfig;
pub use hearthd_config_derive::TryFromPartial;
pub use located::Located;
pub use scalar::scalar;

/// Trait that associates a config type with its partial variant.
///
/// This trait is automatically implemented by the `MergeableConfig` and `SubConfig`
/// derive macros. It allows the macro-generated code to reference the partial type
/// without requiring it to be directly in scope.
pub trait HasPartialConfig {
    /// The partial configuration type (where all fields are optional and wrapped in Spanned)
    type PartialConfig;
}
