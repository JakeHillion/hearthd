use proc_macro2::TokenStream;
use quote::quote;
use syn::Attribute;
use syn::Data;
use syn::DataStruct;
use syn::DeriveInput;
use syn::Error;
use syn::Fields;
use syn::Result;
use syn::ext::IdentExt;

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "FromConfig cannot be derived for a generic struct",
        ));
    }
    let Data::Struct(DataStruct {
        fields: Fields::Named(fields),
        ..
    }) = &input.data
    else {
        return Err(Error::new_spanned(
            name,
            "FromConfig can only be derived for a struct with named fields",
        ));
    };
    reject_config_attributes(&input.attrs)?;

    let mut reads = Vec::new();
    let mut names = Vec::new();
    for field in &fields.named {
        reject_config_attributes(&field.attrs)?;

        let ident = field.ident.as_ref().unwrap();
        let key = ident.unraw().to_string();
        let ty = &field.ty;
        reads.push(quote! {
            let #ident = <#ty as hearthd_config::FromConfig>::from_config(
                tables.field(#key),
                context,
            );
        });
        names.push(ident);
    }

    Ok(quote! {
        impl hearthd_config::FromConfig for #name {
            fn from_config(
                definitions: hearthd_config::Definitions<'_>,
                context: &mut hearthd_config::Context,
            ) -> Option<Self> {
                let tables = definitions.tables(context)?;
                #(#reads)*
                Some(Self { #(#names: #names?,)* })
            }
        }
    })
}

fn reject_config_attributes(attrs: &[Attribute]) -> Result<()> {
    match attrs.iter().find(|attr| attr.path().is_ident("config")) {
        Some(attr) => Err(Error::new_spanned(
            attr,
            "FromConfig does not understand this attribute",
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(source: &str) -> String {
        expand(syn::parse_str(source).unwrap())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn a_struct_reads_each_field_by_name() {
        let input = syn::parse_str("struct Config { http: Http, r#type: Kind }").unwrap();
        let output = expand(input).unwrap().to_string();

        assert!(output.contains("tables . field (\"http\")"), "{output}");
        assert!(output.contains("tables . field (\"type\")"), "{output}");
    }

    #[test]
    fn only_plain_structs_are_accepted() {
        assert_eq!(
            error("struct Config<T> { value: T }"),
            "FromConfig cannot be derived for a generic struct"
        );
        assert_eq!(
            error("enum Config { A }"),
            "FromConfig can only be derived for a struct with named fields"
        );
        assert_eq!(
            error("struct Config(u8);"),
            "FromConfig can only be derived for a struct with named fields"
        );
    }

    #[test]
    fn unknown_attributes_are_rejected() {
        assert_eq!(
            error("struct Config { #[config(no_such_option)] value: Value }"),
            "FromConfig does not understand this attribute"
        );
    }
}
