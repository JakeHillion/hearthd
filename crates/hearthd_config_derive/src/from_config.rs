use proc_macro2::TokenStream;
use quote::quote;
use syn::Attribute;
use syn::Data;
use syn::DataStruct;
use syn::DeriveInput;
use syn::Error;
use syn::Expr;
use syn::ExprLit;
use syn::Field;
use syn::Fields;
use syn::Lit;
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
        .map(|field| Ok((field, Options::of(field)?)))
        .collect::<Result<Vec<_>>>()?;
    if let Some((second, _)) = fields.iter().filter(|(_, o)| o.flatten).nth(1) {
        return Err(Error::new_spanned(
            second,
            "only one field can take the keys the others leave",
        ));
    }
    let named: Vec<String> = fields
        .iter()
        .filter(|(_, options)| !options.flatten)
        .map(|(field, _)| key(field))
        .collect();

    let mut reads = Vec::new();
    let mut names = Vec::new();
    for (field, options) in &fields {
        let ident = field.ident.as_ref().unwrap();
        let ty = &field.ty;
        let definitions = if options.flatten {
            quote! { tables.rest(&[#(#named),*]) }
        } else {
            let key = key(field);
            quote! { tables.field(#key) }
        };
        let read = quote! {
            <#ty as hearthd_config::FromConfig>::from_config(definitions, context)
        };
        let read = match &options.default {
            Some(default) => {
                // A string literal stands for whatever owned type the field
                // is; anything else has to be the field's type already.
                let default = match default {
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(_), ..
                    }) => quote! { ::core::convert::Into::into(#default) },
                    _ => quote! { #default },
                };
                quote! {
                    if definitions.is_absent() {
                        let default: #ty = #default;
                        Some(default)
                    } else {
                        #read
                    }
                }
            }
            None => read,
        };
        reads.push(quote! {
            let #ident = {
                let definitions = #definitions;
                #read
            };
        });
        names.push(ident);
    }

    // A flattened map reads every key the fields leave, so there is nothing
    // left over to warn about.
    let unknown = if fields.iter().any(|(_, options)| options.flatten) {
        quote! {}
    } else {
        quote! { tables.warn_unknown(&[#(#named),*], context); }
    };

    Ok(quote! {
        impl hearthd_config::FromConfig for #name {
            fn from_config(
                definitions: hearthd_config::Definitions<'_>,
                context: &mut hearthd_config::Context,
            ) -> Option<Self> {
                let tables = definitions.tables(context)?;
                #(#reads)*
                #unknown
                Some(Self { #(#names: #names?,)* })
            }
        }
    })
}

/// The key a field is read from.
fn key(field: &Field) -> String {
    field.ident.as_ref().unwrap().unraw().to_string()
}

/// What `#[config(...)]` asks for on one field.
#[derive(Default)]
struct Options {
    /// A map that takes every key the struct's other fields do not.
    flatten: bool,
    /// The value when no file sets the field.
    default: Option<Expr>,
}

impl Options {
    fn of(field: &Field) -> Result<Self> {
        let mut options = Self::default();
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("config")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("flatten") {
                    options.flatten = true;
                    Ok(())
                } else if meta.path.is_ident("default") {
                    options.default = Some(meta.value()?.parse()?);
                    Ok(())
                } else {
                    Err(meta.error("FromConfig does not understand this attribute"))
                }
            })?;
        }

        if options.flatten && options.default.is_some() {
            return Err(Error::new_spanned(
                field,
                "a field that takes the remaining keys cannot have a default",
            ));
        }
        Ok(options)
    }
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
    fn a_default_is_used_when_the_field_is_absent() {
        let input = syn::parse_str(
            "struct Http { #[config(default = \"localhost\")] host: String, #[config(default = 80)] port: u16 }",
        )
        .unwrap();
        let output = expand(input).unwrap().to_string();

        assert!(
            output.contains(
                "let default : String = :: core :: convert :: Into :: into (\"localhost\")"
            ),
            "{output}"
        );
        assert!(output.contains("let default : u16 = 80"), "{output}");
        assert_eq!(
            error("struct S { #[config(flatten, default = 1)] a: Map }"),
            "a field that takes the remaining keys cannot have a default"
        );
    }

    #[test]
    fn keys_no_field_reads_are_checked_unless_a_field_takes_them() {
        let plain = syn::parse_str("struct Http { host: String, port: u16 }").unwrap();
        let output = expand(plain).unwrap().to_string();
        assert!(
            output.contains("tables . warn_unknown (& [\"host\" , \"port\"] , context)"),
            "{output}"
        );

        let flattened = syn::parse_str("struct Hosts { #[config(flatten)] hosts: Map }").unwrap();
        let output = expand(flattened).unwrap().to_string();
        assert!(!output.contains("warn_unknown"), "{output}");
    }

    #[test]
    fn unknown_attributes_are_rejected() {
        assert_eq!(
            error("struct Config { #[config(no_such_option)] value: Value }"),
            "FromConfig does not understand this attribute"
        );
    }
}
