use crate::Diagnostic;

/// Trait for validating cross-field constraints in configuration.
///
/// This trait provides a hook for validating relationships between fields
/// that cannot be checked during parsing or field-level validation. For
/// example, validating that a reference to another field actually exists.
///
/// The `validate()` method is automatically called by `MergeableConfig::from_files()`
/// after the config is loaded and converted from its partial form.
///
/// ## Example
///
/// ```ignore
/// impl Validate for Config {
///     fn validate(&self) -> Vec<Diagnostic> {
///         let mut diagnostics = Vec::new();
///
///         if let Some(ref default) = self.locations.default {
///             if !self.locations.locations.contains_key(default) {
///                 diagnostics.push(Diagnostic::Error(Error::Validation(ValidationError {
///                     field_path: "locations.default".to_string(),
///                     message: format!("default location '{}' not found", default),
///                     span: None,
///                     source: None,
///                 })));
///             }
///         }
///
///         diagnostics
///     }
/// }
/// ```
pub trait Validate {
    /// Validate cross-field constraints.
    ///
    /// Returns a vector of diagnostics (errors or warnings) found during
    /// validation. The default implementation performs no validation and
    /// returns an empty vector.
    fn validate(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}
