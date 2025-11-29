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
