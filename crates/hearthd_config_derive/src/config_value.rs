use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;
use syn::Error;
use syn::Result;

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "ConfigValue cannot be derived for a generic type",
        ));
    }

    Ok(quote! {
        impl hearthd_config::FromConfig for #name {
            fn from_config(
                definitions: hearthd_config::Definitions<'_>,
                context: &mut hearthd_config::Context,
            ) -> Option<Self> {
                hearthd_config::scalar(definitions, context)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_read_as_a_scalar() {
        let input = syn::parse_str("enum Level { Info, Debug }").unwrap();
        let output = expand(input).unwrap().to_string();

        assert!(
            output.contains("impl hearthd_config :: FromConfig for Level"),
            "{output}"
        );
        assert!(output.contains("hearthd_config :: scalar"), "{output}");
    }

    #[test]
    fn generic_types_are_rejected() {
        let input = syn::parse_str("struct Wrapper<T>(T);").unwrap();

        assert_eq!(
            expand(input).unwrap_err().to_string(),
            "ConfigValue cannot be derived for a generic type"
        );
    }
}
