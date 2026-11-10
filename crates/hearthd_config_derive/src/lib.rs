use proc_macro::TokenStream;
use syn::DeriveInput;
use syn::parse_macro_input;

mod config_value;
mod from_config;

/// Derive `FromConfig` for a struct, reading each field from the key of the
/// same name.
///
/// A table written across several files is read as one: every field sees
/// each place it was set, whichever file that was in.
///
/// # Attributes
///
/// - `#[config(default = <expr>)]`: the field's value when no file sets it.
///   A string literal is converted with `Into`, so it can stand for a
///   `String`.
/// - `#[config(flatten)]`: on one map field, which then takes every key the
///   struct's other fields do not read.
#[proc_macro_derive(FromConfig, attributes(config))]
pub fn derive_from_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match from_config::expand(input) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive `FromConfig` for a type that is written as a single value, such
/// as an enum or a string that is parsed.
///
/// The type says how it is read by implementing `serde::Deserialize`. It is
/// then required wherever it appears, cannot be set in two files, and a
/// value serde rejects is reported where it is written.
#[proc_macro_derive(ConfigValue)]
pub fn derive_config_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match config_value::expand(input) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
