use std::path::Path;
use std::path::PathBuf;

use crate::Diagnostic;
use crate::Diagnostics;
use crate::HasPartialConfig;
use crate::LoadError;
use crate::TryFromPartial;
use crate::Validate;

/// Trait for partial configuration structs that can be loaded and merged.
///
/// This trait is automatically implemented by the `MergeableConfig` derive macro
/// for the generated `Partial{TypeName}` structs. It provides methods for loading
/// configuration from files and merging multiple partial configs together.
pub trait PartialMergeableConfig: Sized {
    /// Load a single TOML file into a partial configuration.
    ///
    /// This reads the file, parses it as TOML, and attaches source information
    /// to all `Located<T>` fields for error reporting.
    fn from_file(path: &Path) -> Result<Self, LoadError>;

    /// Load multiple TOML files with recursive import resolution.
    ///
    /// Files are loaded in the order specified, with imports processed recursively.
    /// Import cycles are detected and reported as errors. Relative import paths
    /// are resolved relative to the file containing the `imports` field.
    ///
    /// Returns a vector of all loaded partial configs (including imported files).
    fn load_with_imports(paths: &[PathBuf]) -> Result<Vec<Self>, LoadError>;

    /// Merge multiple partial configurations into a single partial config.
    ///
    /// Merging behavior depends on field types:
    /// - Simple fields: First value wins, later assignments are conflicts
    /// - `HashMap<K, SimpleValue>`: Keys can be defined in multiple files, conflicts per key
    /// - `HashMap<K, Struct>`: Structs with same key are merged field-by-field recursively
    /// - Nested structs: Merged recursively
    ///
    /// Returns the merged partial config and a vector of diagnostics (warnings and errors).
    /// Empty config files generate warnings.
    fn merge<I>(configs: I) -> (Self, Vec<Diagnostic>)
    where
        I: IntoIterator<Item = Self>;
}

/// Trait for root configuration structs that can be loaded from files.
///
/// This trait is automatically implemented by the `MergeableConfig` derive macro.
/// It provides a complete workflow for loading, merging, validating, and building
/// configuration from TOML files.
///
/// The `from_files()` method orchestrates the entire process:
/// 1. Load files with import resolution (`PartialConfig::load_with_imports`)
/// 2. Merge partial configs (`PartialConfig::merge`)
/// 3. Convert from partial to final config (`TryFromPartial::try_from_partial`)
/// 4. Validate cross-field constraints (`Validate::validate`)
/// 5. Return result based on error status
///
/// ## Example
///
/// ```ignore
/// #[derive(MergeableConfig, TryFromPartial)]
/// struct Config {
///     port: u16,
///     host: String,
/// }
///
/// // Automatically gets this method:
/// let (config, diagnostics) = Config::from_files(&[PathBuf::from("config.toml")])?;
/// ```
pub trait MergeableConfig: Sized + Default + TryFromPartial + Validate + HasPartialConfig
where
    Self::PartialConfig: PartialMergeableConfig,
{
    /// Load configuration from multiple TOML files with full validation.
    ///
    /// This method performs the complete configuration loading workflow:
    /// - Loads files with recursive import resolution
    /// - Merges all partial configs
    /// - Validates required fields and types
    /// - Validates cross-field constraints
    /// - Collects all diagnostics (warnings and errors)
    ///
    /// Returns `Ok((config, diagnostics))` if no errors occurred (warnings are OK),
    /// or `Err(diagnostics)` if any validation errors were found. In the error case,
    /// a default config is used for error recovery, allowing multiple errors to be
    /// reported at once.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// match Config::from_files(&[PathBuf::from("config.toml")]) {
    ///     Ok((config, diagnostics)) => {
    ///         // Config loaded successfully, might have warnings
    ///         println!("{}", format_diagnostics(&diagnostics.0));
    ///         // Use config...
    ///     }
    ///     Err(diagnostics) => {
    ///         // One or more errors occurred
    ///         eprintln!("{}", format_diagnostics(&diagnostics.0));
    ///         std::process::exit(1);
    ///     }
    /// }
    /// ```
    fn from_files(paths: &[PathBuf]) -> Result<(Self, Diagnostics), Diagnostics> {
        // Step 1: Load files with import resolution
        let configs = <Self::PartialConfig as PartialMergeableConfig>::load_with_imports(paths)
            .map_err(|e| Diagnostics(vec![Diagnostic::Error(crate::Error::Load(e))]))?;

        // Step 2: Merge partial configs
        let (partial, mut diagnostics) =
            <Self::PartialConfig as PartialMergeableConfig>::merge(configs);

        // Step 3: Convert from partial to final config
        let config = match Self::try_from_partial(partial) {
            Ok(cfg) => cfg,
            Err(errs) => {
                diagnostics.extend(errs);
                Self::default() // Error recovery: use default
            }
        };

        // Step 4: Validate cross-field constraints
        diagnostics.extend(config.validate());

        // Step 5: Return result based on error status
        let has_errors = diagnostics.iter().any(|d| d.is_error());
        if has_errors {
            Err(Diagnostics(diagnostics))
        } else {
            Ok((config, Diagnostics(diagnostics)))
        }
    }
}
