use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod generate;

#[proc_macro_derive(MergeableConfig, attributes(config))]
pub fn derive_mergeable_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::expand_mergeable_config(input, true) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_derive(SubConfig, attributes(config))]
pub fn derive_sub_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::expand_mergeable_config(input, false) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
