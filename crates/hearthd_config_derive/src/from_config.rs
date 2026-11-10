use proc_macro2::TokenStream;
use quote::quote;
use syn::Attribute;
use syn::Data;
use syn::DataStruct;
use syn::DeriveInput;
use syn::Error;
use syn::Field;
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

    let fields = fields
        .named
        .iter()
        .map(|field| Ok((field, is_flattened(field)?)))
        .collect::<Result<Vec<_>>>()?;
    if let Some((second, _)) = fields.iter().filter(|(_, flattened)| *flattened).nth(1) {
        return Err(Error::new_spanned(
            second,
            "only one field can take the keys the others leave",
        ));
    }
    let named: Vec<String> = fields
        .iter()
        .filter(|(_, flattened)| !flattened)
        .map(|(field, _)| key(field))
        .collect();

    let mut reads = Vec::new();
    let mut names = Vec::new();
    for (field, flattened) in &fields {
        let ident = field.ident.as_ref().unwrap();
        let ty = &field.ty;
        let definitions = if *flattened {
            quote! { tables.rest(&[#(#named),*]) }
        } else {
            let key = key(field);
            quote! { tables.field(#key) }
        };
        reads.push(quote! {
            let #ident = <#ty as hearthd_config::FromConfig>::from_config(
                #definitions,
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

/// The key a field is read from.
fn key(field: &Field) -> String {
    field.ident.as_ref().unwrap().unraw().to_string()
}

/// Whether a field is marked `#[config(flatten)]`: a map that takes every
/// key the struct's other fields do not.
fn is_flattened(field: &Field) -> Result<bool> {
    let mut flattened = false;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("config")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("flatten") {
                flattened = true;
                Ok(())
            } else {
                Err(meta.error("FromConfig does not understand this attribute"))
            }
        })?;
    }
    Ok(flattened)
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
    fn a_flattened_field_takes_the_keys_the_others_leave() {
        let input = syn::parse_str(
            "struct Locations { default: Name, #[config(flatten)] rest: Map, other: Name }",
        )
        .unwrap();
        let output = expand(input).unwrap().to_string();

        assert!(
            output.contains("tables . rest (& [\"default\" , \"other\"])"),
            "{output}"
        );
        assert_eq!(
            error("struct S { #[config(flatten)] a: Map, #[config(flatten)] b: Map }"),
            "only one field can take the keys the others leave"
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
