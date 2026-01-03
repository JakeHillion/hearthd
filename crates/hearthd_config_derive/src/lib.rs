use proc_macro::TokenStream;
use syn::DeriveInput;
use syn::parse_macro_input;

mod generate;

/// Derive macro for root configuration structs that can be loaded from files and merged.
///
/// This macro generates a `Partial{TypeName}` struct and several methods for loading,
/// merging, and validating configuration from multiple TOML files.
///
/// # Generated Code
///
/// - `Partial{TypeName}`: A version of your struct where all fields are `Option<T>` and
///   wrapped in `toml::Spanned<T>` for source location tracking
/// - `from_file(path)`: Load a single TOML file
/// - `load_with_imports(paths)`: Load multiple files with recursive import resolution
/// - `merge(configs)`: Merge multiple partial configs with conflict detection
///
/// # Attributes
///
/// - `#[config(no_span)]`: Disable `Spanned` wrapping for this struct (useful for types
///   used as HashMap values where span tracking isn't needed)
/// - `#[config(default = "function_name")]`: Specify a default function for a required field.
///   The function will be called if the field is missing from the config. No validation error
///   will be generated for missing fields with defaults.
///
/// # Example
///
/// ```ignore
/// use hearthd_config::MergeableConfig;
///
/// #[derive(MergeableConfig, Deserialize)]
/// struct Config {
///     port: u16,
///     host: String,
///     #[serde(flatten)]
///     locations: HashMap<String, Location>,
/// }
///
/// #[derive(SubConfig, Deserialize)]
/// #[config(no_span)]
/// struct Location {
///     latitude: f64,
///     longitude: f64,
/// }
///
/// let configs = PartialConfig::load_with_imports(&["config.toml"])?;
/// let merged = PartialConfig::merge(configs)?;
/// let config: Config = merged.try_into()?;
/// ```
#[proc_macro_derive(MergeableConfig, attributes(config))]
pub fn derive_mergeable_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::expand_mergeable_config(input, true) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive macro for nested configuration structs that support field-level merging.
///
/// This macro generates the same `Partial{TypeName}` struct and `merge_from` method
/// as `MergeableConfig`, but without file loading capabilities. Use this for nested
/// configuration types that aren't directly loaded from files.
///
/// # Generated Code
///
/// - `Partial{TypeName}`: A version of your struct where all fields are `Option<T>` and
///   wrapped in `toml::Spanned<T>` for source location tracking
/// - `merge_from(other, path)`: Merge another partial config into this one with conflict detection
///
/// # Merging Behavior
///
/// - Simple fields: First value wins, later assignments are conflicts
/// - `HashMap<K, SimpleValue>`: Keys can be defined in multiple files, conflicts per key
/// - `HashMap<K, Struct>`: Structs with same key are merged field-by-field recursively
/// - Nested structs: Merged recursively
///
/// # Attributes
///
/// - `#[config(no_span)]`: Disable `Spanned` wrapping for this struct
/// - `#[config(default = "function_name")]`: Specify a default function for a required field.
///   The function will be called if the field is missing from the config.
/// - `#[serde(flatten)]`: Mark HashMap fields that are flattened in the parent struct
///
/// # Example
///
/// ```ignore
/// use hearthd_config::SubConfig;
///
/// #[derive(SubConfig, Deserialize)]
/// struct HttpConfig {
///     port: u16,
///     timeout_ms: Option<u32>,
/// }
/// ```
#[proc_macro_derive(SubConfig, attributes(config))]
pub fn derive_sub_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::expand_mergeable_config(input, false) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive macro for implementing `TryFromPartial` trait for config validation.
///
/// This macro automatically generates the conversion logic from `Partial{TypeName}` to
/// `{TypeName}` with comprehensive validation and error recovery.
///
/// # Generated Code
///
/// - `impl TryFromPartial for {TypeName}`: Converts partial config to final config
/// - Validates required fields and collects all errors
/// - Recursively calls `try_from_partial` on nested structs
/// - Prepends field paths to nested errors for accurate reporting
///
/// # Field Handling
///
/// - **Simple fields**: Unwraps `Located<T>`, validates if required
/// - **Fields with defaults**: Uses `#[config(default = "fn")]` to call the default function
///   when the field is missing. No validation error is generated.
/// - **Optional fields**: Uses `.map()` to unwrap, no error if missing
/// - **HashMap of simple values**: Maps over entries, unwraps each value
/// - **HashMap of structs**: Recursively calls `try_from_partial`, prepends key to errors
/// - **Nested structs**: Recursively calls `try_from_partial`, prepends field name to errors
///
/// # Attributes
///
/// - `#[config(default = "function_name")]`: Specify a default function for a required field.
///   The function must return the field's type and will be called if the field is missing.
///
/// # Example
///
/// ```ignore
/// use hearthd_config::{TryFromPartial, SubConfig};
///
/// fn default_port() -> u16 {
///     8080
/// }
///
/// #[derive(TryFromPartial, SubConfig)]
/// struct HttpConfig {
///     #[config(default = "default_port")]
///     port: u16,              // Uses default_port() if missing
///     host: String,           // Required, error if missing
///     timeout: Option<u32>,   // Optional, no error if missing
/// }
///
/// // If config only has "host = 'localhost'":
/// // - port will be 8080 (from default_port())
/// // - host will be "localhost"
/// // - timeout will be None
/// ```
#[proc_macro_derive(TryFromPartial, attributes(config))]
pub fn derive_try_from_partial(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::expand_try_from_partial(input) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
