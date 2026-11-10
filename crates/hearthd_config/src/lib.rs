mod diagnostics;
mod document;
mod from_config;
mod scalar;
mod validate;

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
pub use from_config::Context;
pub use from_config::Definitions;
pub use from_config::FromConfig;
pub use from_config::Tables;
pub use from_config::from_sources;
pub use from_config::load;
pub use hearthd_config_derive::ConfigValue;
pub use hearthd_config_derive::FromConfig;
pub use scalar::scalar;
pub use validate::Validate;
pub use validate::Validation;

/// What the derives expand to. Not for use by hand.
#[doc(hidden)]
pub mod __private {
    pub use crate::validate::Probe;
    pub use crate::validate::WithValidate;
    pub use crate::validate::WithoutValidate;
}
