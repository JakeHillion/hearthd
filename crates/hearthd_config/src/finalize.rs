use crate::Diagnostic;
use crate::HasPartialConfig;

/// Trait for converting from partial config to final config with validation
///
/// This trait provides a standard way to convert from `PartialConfig` types
/// (where all fields are `Option<Located<T>>`) to final config types (where
/// required fields are unwrapped and validated).
///
/// Unlike `TryFrom`, this trait:
/// - Returns `Vec<Diagnostic>` for collecting multiple validation errors
/// - Works recursively through nested configs
/// - Supports field path construction via `Diagnostic::prepend_path()`
///
/// ## Example
///
/// ```ignore
/// impl TryFromPartial for MyConfig {
///     fn try_from_partial(partial: PartialMyConfig) -> Result<Self, Vec<Diagnostic>> {
///         let mut diagnostics = Vec::new();
///
///         // Validate required fields
///         let name = if let Some(n) = partial.name {
///             n.into_inner()
///         } else {
///             diagnostics.push(Diagnostic::Error(Error::Validation(ValidationError {
///                 field_path: "name".to_string(),
///                 message: "name is required".to_string(),
///                 span: None,
///                 source: partial.source.clone(),
///             })));
///             String::new() // default for error recovery
///         };
///
///         if diagnostics.is_empty() {
///             Ok(MyConfig { name })
///         } else {
///             Err(diagnostics)
///         }
///     }
/// }
/// ```
pub trait TryFromPartial: Sized + HasPartialConfig {
    /// Convert from partial config to final config with validation
    ///
    /// Returns `Ok(Self)` if all validation passes, or `Err(Vec<Diagnostic>)`
    /// if any validation errors occurred. Implementations should collect all
    /// errors before returning to provide comprehensive feedback.
    fn try_from_partial(partial: Self::PartialConfig) -> Result<Self, Vec<Diagnostic>>;
}
