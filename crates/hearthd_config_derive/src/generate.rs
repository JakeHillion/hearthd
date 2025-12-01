use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::Data;
use syn::DeriveInput;
use syn::Error;
use syn::Field;
use syn::Fields;
use syn::GenericArgument;
use syn::Ident;
use syn::PathArguments;
use syn::Result;
use syn::Type;
use syn::TypePath;

struct FieldInfo {
    name: Ident,
    field_type: FieldType,
    flattened: bool,
    default_fn: Option<String>,
}

enum FieldType {
    Simple(#[allow(dead_code)] Type),
    HashMap {
        key_type: Type,
        #[allow(dead_code)]
        value_type: Type,
    },
    HashMapOfStructs {
        key_type: Type,
        #[allow(dead_code)]
        value_type: Type,
    },
    Nested(#[allow(dead_code)] Type),
}

pub fn expand_mergeable_config(input: DeriveInput, is_root: bool) -> Result<TokenStream> {
    let name = &input.ident;

    // Check for #[config(no_span)] attribute to override span tracking
    let no_span = input.attrs.iter().any(|attr| {
        if attr.path().is_ident("config") {
            if let Ok(syn::Meta::Path(path)) = attr.parse_args::<syn::Meta>() {
                return path.is_ident("no_span");
            }
        }
        false
    });

    // Use spans unless explicitly disabled with #[config(no_span)]
    let use_spans = !no_span;

    // Only support structs
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(Error::new_spanned(
                    name,
                    "MergeableConfig only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(Error::new_spanned(
                name,
                "MergeableConfig only supports structs",
            ));
        }
    };

    // Analyze fields
    let mut field_infos = Vec::new();
    for field in fields {
        let field_name = field.ident.as_ref().unwrap().clone();
        let field_ty = &field.ty;

        // Check if field has #[serde(flatten)] attribute
        let flattened = field.attrs.iter().any(|attr| {
            if attr.path().is_ident("serde") {
                if let Ok(syn::Meta::Path(path)) = attr.parse_args::<syn::Meta>() {
                    return path.is_ident("flatten");
                }
            }
            false
        });

        // Parse default function if present
        let default_fn = parse_default_fn(field);

        let field_type = if is_hashmap(field_ty) {
            let (key_type, value_type) = extract_hashmap_types(field_ty)?;
            // Check if value type is a struct (not a simple type)
            if is_simple_type(&value_type) {
                FieldType::HashMap {
                    key_type,
                    value_type,
                }
            } else {
                FieldType::HashMapOfStructs {
                    key_type,
                    value_type,
                }
            }
        } else if is_simple_type(field_ty) {
            FieldType::Simple(field_ty.clone())
        } else if let Some(inner_ty) = is_option_type(field_ty) {
            // Option<T> where T is simple should be treated as Simple
            if is_simple_type(&inner_ty) {
                FieldType::Simple(field_ty.clone())
            } else {
                // Option<SomeStruct> is still Nested
                FieldType::Nested(field_ty.clone())
            }
        } else {
            FieldType::Nested(field_ty.clone())
        };

        field_infos.push(FieldInfo {
            name: field_name,
            field_type,
            flattened,
            default_fn,
        });
    }

    // Generate code
    let partial_struct = generate_partial_struct(name, fields, use_spans)?;
    let merge_impl = if is_root {
        generate_root_merge_impl(name, &field_infos, use_spans)?
    } else {
        generate_sub_merge_impl(name, &field_infos, use_spans)?
    };
    let load_impl = if is_root {
        Some(generate_load_impl(name)?)
    } else {
        None
    };
    // TryFrom and from_files are implemented manually to handle validation
    let try_from_impl: Option<TokenStream> = None;
    let config_impl: Option<TokenStream> = None;

    let attach_source_impl = generate_attach_source_impl(name, &field_infos, use_spans)?;

    Ok(quote! {
        #partial_struct
        #attach_source_impl
        #merge_impl
        #load_impl
        #try_from_impl
        #config_impl
    })
}

fn generate_partial_struct(
    config_name: &Ident,
    fields: &syn::punctuated::Punctuated<Field, syn::token::Comma>,
    use_spans: bool,
) -> Result<TokenStream> {
    let partial_name = format_ident!("Partial{}", config_name);

    let mut partial_fields = Vec::new();
    for field in fields {
        let name = &field.ident;
        let field_ty = &field.ty;

        // Check if field has #[serde(flatten)] attribute
        let has_flatten = field.attrs.iter().any(|attr| {
            if attr.path().is_ident("serde") {
                if let Ok(syn::Meta::Path(path)) = attr.parse_args::<syn::Meta>() {
                    return path.is_ident("flatten");
                }
            }
            false
        });

        let field_decl = if is_hashmap(field_ty) {
            let (key_type, value_type) = extract_hashmap_types(field_ty)?;
            if is_simple_type(&value_type) {
                // Only use Located if use_spans is true
                if use_spans {
                    quote! { std::collections::HashMap<#key_type, hearthd_config::Located<#value_type>> }
                } else {
                    quote! { std::collections::HashMap<#key_type, #value_type> }
                }
            } else {
                quote! { std::collections::HashMap<#key_type, <#value_type as hearthd_config::HasPartialConfig>::PartialConfig> }
            }
        } else if let Some(inner_ty) = is_option_type(field_ty) {
            // Option<T> - only use Located if use_spans is true
            if is_simple_type(&inner_ty) {
                if use_spans {
                    quote! { hearthd_config::Located<#inner_ty> }
                } else {
                    quote! { #inner_ty }
                }
            } else {
                // Option<ComplexStruct> - use PartialConfig
                quote! { <#inner_ty as hearthd_config::HasPartialConfig>::PartialConfig }
            }
        } else if is_simple_type(field_ty) {
            // Only use Located if use_spans is true
            if use_spans {
                quote! { hearthd_config::Located<#field_ty> }
            } else {
                quote! { #field_ty }
            }
        } else {
            quote! { <#field_ty as hearthd_config::HasPartialConfig>::PartialConfig }
        };

        let field_tokens = if has_flatten {
            quote! {
                #[serde(flatten)]
                pub #name: #field_decl
            }
        } else {
            quote! {
                pub #name: Option<#field_decl>
            }
        };

        partial_fields.push(field_tokens);
    }

    Ok(quote! {
        #[derive(Debug, Default, serde::Deserialize)]
        pub struct #partial_name {
            #[serde(default)]
            pub imports: Vec<String>,

            #(#partial_fields,)*

            #[serde(skip)]
            pub source: Option<hearthd_config::SourceInfo>,
        }

        impl hearthd_config::HasPartialConfig for #config_name {
            type PartialConfig = #partial_name;
        }
    })
}

fn generate_root_merge_impl(
    config_name: &Ident,
    fields: &[FieldInfo],
    use_spans: bool,
) -> Result<TokenStream> {
    let partial_name = format_ident!("Partial{}", config_name);

    // Collect all HashMap key types for trait bounds
    let mut key_types: Vec<&Type> = Vec::new();
    for field in fields {
        match &field.field_type {
            FieldType::HashMap { key_type, .. } | FieldType::HashMapOfStructs { key_type, .. } => {
                key_types.push(key_type);
            }
            _ => {}
        }
    }

    // Generate tracking variables
    let tracking_vars: Vec<_> = fields
        .iter()
        .map(|f| {
            let name = &f.name;
            match &f.field_type {
                FieldType::HashMap { key_type, .. } => {
                    let var_name = format_ident!("{}_locs", name);
                    quote! {
                        let mut #var_name: std::collections::HashMap<#key_type, hearthd_config::MergeConflictLocation> = std::collections::HashMap::new();
                    }
                }
                FieldType::HashMapOfStructs { key_type, .. } => {
                    let var_name = format_ident!("{}_field_locs", name);
                    quote! {
                        let mut #var_name: std::collections::HashMap<#key_type, std::collections::HashMap<String, hearthd_config::MergeConflictLocation>> = std::collections::HashMap::new();
                    }
                }
                FieldType::Nested(_) => {
                    let var_name = format_ident!("{}_field_locs", name);
                    quote! {
                        let mut #var_name: std::collections::HashMap<(), std::collections::HashMap<String, hearthd_config::MergeConflictLocation>> = std::collections::HashMap::new();
                    }
                }
                _ => {
                    let var_name = format_ident!("{}_loc", name);
                    quote! {
                        let mut #var_name: Option<hearthd_config::MergeConflictLocation> = None;
                    }
                }
            }
        })
        .collect();

    // Generate merge logic for each field
    let merge_logic: Vec<_> = fields
        .iter()
        .map(|f| generate_field_merge(f, use_spans))
        .collect::<Result<Vec<_>>>()?;

    // Generate empty check
    let empty_checks: Vec<_> = fields
        .iter()
        .map(|f| {
            let name = &f.name;
            quote! { config.#name.is_none() }
        })
        .collect();

    // Generate where clause for HashMap keys if needed
    let where_clause_bounds = if !key_types.is_empty() {
        quote! { #(#key_types: std::fmt::Display + std::hash::Hash + Eq + Clone,)* }
    } else {
        quote! {}
    };

    Ok(quote! {
        impl #partial_name {
            pub fn merge<I>(configs: I) -> (Self, Vec<hearthd_config::Diagnostic>)
            where
                I: IntoIterator<Item = Self>,
                #where_clause_bounds
            {
                let mut result = Self::default();
                let mut diagnostics = Vec::new();
                let mut imports = Vec::new();

                #(#tracking_vars)*

                for config in configs {
                    imports.extend(config.imports.clone());

                    let source_info = config.source.as_ref().cloned().unwrap_or_else(|| hearthd_config::SourceInfo {
                        file_path: std::path::PathBuf::from("<unknown>"),
                        content: String::new(),
                    });

                    let is_empty = #(#empty_checks)&&* && config.imports.is_empty();

                    if is_empty {
                        diagnostics.push(hearthd_config::Diagnostic::Warning(hearthd_config::Warning::EmptyConfig {
                            file_path: source_info.file_path.clone(),
                        }));
                    }

                    #(#merge_logic)*
                }

                result.imports = imports;
                (result, diagnostics)
            }
        }
    })
}

fn generate_sub_merge_impl(
    config_name: &Ident,
    fields: &[FieldInfo],
    use_spans: bool,
) -> Result<TokenStream> {
    let partial_name = format_ident!("Partial{}", config_name);

    // Collect all HashMap key types for trait bounds
    let mut key_types: Vec<&Type> = Vec::new();
    for field in fields {
        match &field.field_type {
            FieldType::HashMap { key_type, .. } | FieldType::HashMapOfStructs { key_type, .. } => {
                key_types.push(key_type);
            }
            _ => {}
        }
    }

    // Generate field-level merge logic
    let merge_fields: Vec<_> = fields
        .iter()
        .map(|f| generate_sub_field_merge(f, use_spans))
        .collect::<Result<Vec<_>>>()?;

    // Generate where clause for HashMap keys if needed
    let where_clause = if !key_types.is_empty() {
        quote! { where #(#key_types: std::fmt::Display + std::hash::Hash + Eq + Clone),* }
    } else {
        quote! {}
    };

    Ok(quote! {
        impl #partial_name #where_clause {
            /// Merge another partial config into this one, tracking conflicts
            #[allow(clippy::ptr_arg)]
            pub fn merge_from(
                &mut self,
                mut other: Self,
                field_locs: &mut std::collections::HashMap<String, hearthd_config::MergeConflictLocation>,
                source_info: &hearthd_config::SourceInfo,
                field_prefix: &str,
                diagnostics: &mut Vec<hearthd_config::Diagnostic>,
            ) {
                #(#merge_fields)*
            }
        }
    })
}

fn generate_sub_field_merge(field: &FieldInfo, use_spans: bool) -> Result<TokenStream> {
    let name = &field.name;
    let name_str = name.to_string();

    match &field.field_type {
        FieldType::Simple(_) => {
            if use_spans {
                // For Spanned types, detect conflicts
                Ok(quote! {
                    if let Some(value) = std::mem::take(&mut other.#name) {
                        if self.#name.is_none() {
                            // First occurrence - just record it
                            let conflict_loc = value.to_conflict_location();
                            self.#name = Some(value);
                            field_locs.insert(#name_str.to_string(), conflict_loc);
                        } else {
                            // Conflict detected - field already set
                            let field_path = format!("{}.{}", field_prefix, #name_str);
                            let first_loc = field_locs.get(#name_str).cloned().unwrap_or_else(|| {
                                // Fallback: extract location from the existing Located value
                                self.#name.as_ref().unwrap().to_conflict_location()
                            });
                            let conflict_loc = value.to_conflict_location();
                            let message = format!("Field '{}' defined in multiple config files", field_path);
                            diagnostics.push(hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(hearthd_config::MergeError {
                                field_path,
                                message,
                                conflicts: vec![first_loc, conflict_loc],
                            })));
                        }
                    }
                })
            } else {
                // For plain types (no Spanned), still detect conflicts but without span info
                Ok(quote! {
                    if let Some(_value) = std::mem::take(&mut other.#name) {
                        if self.#name.is_some() {
                            // Conflict detected - field already set
                            let field_path = format!("{}.{}", field_prefix, #name_str);
                            let message = format!("Field '{}' defined in multiple config files", field_path);
                            let conflict_loc = hearthd_config::MergeConflictLocation {
                                file_path: source_info.file_path.clone(),
                                span: 0..0, // No span info for plain types
                                content: source_info.content.clone(),
                            };
                            let first_loc = field_locs.get(#name_str).cloned().unwrap_or_else(|| {
                                hearthd_config::MergeConflictLocation {
                                    file_path: std::path::PathBuf::new(),
                                    span: 0..0,
                                    content: String::new(),
                                }
                            });
                            diagnostics.push(hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(hearthd_config::MergeError {
                                field_path,
                                message,
                                conflicts: vec![first_loc, conflict_loc],
                            })));
                        } else {
                            // First occurrence - record it
                            self.#name = Some(_value);
                            let conflict_loc = hearthd_config::MergeConflictLocation {
                                file_path: source_info.file_path.clone(),
                                span: 0..0,
                                content: source_info.content.clone(),
                            };
                            field_locs.insert(#name_str.to_string(), conflict_loc);
                        }
                    }
                })
            }
        }
        FieldType::HashMap { .. } => {
            if use_spans {
                Ok(quote! {
                    if let Some(map) = other.#name {
                        if self.#name.is_none() {
                            self.#name = Some(std::collections::HashMap::new());
                        }

                        let self_map = self.#name.as_mut().unwrap();
                        for (key, value_spanned) in map {
                            let field_path = format!("{}.{}.{}", field_prefix, #name_str, key);
                            let conflict_loc = value_spanned.to_conflict_location();

                            let key_str = key.to_string();
                            if let Some(prev_loc) = field_locs.get(&key_str) {
                                let message = format!("Field '{}' defined in multiple config files", field_path);
                                diagnostics.push(hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(hearthd_config::MergeError {
                                    field_path,
                                    message,
                                    conflicts: vec![prev_loc.clone(), conflict_loc],
                                })));
                            } else {
                                self_map.insert(key.clone(), value_spanned);
                                field_locs.insert(key_str, conflict_loc);
                            }
                        }
                    }
                })
            } else {
                Ok(quote! {
                    if let Some(map) = other.#name {
                        if self.#name.is_none() {
                            self.#name = Some(std::collections::HashMap::new());
                        }

                        let self_map = self.#name.as_mut().unwrap();
                        for (key, value) in map {
                            let field_path = format!("{}.{}.{}", field_prefix, #name_str, key);
                            let conflict_loc = hearthd_config::MergeConflictLocation {
                                file_path: source_info.file_path.clone(),
                                span: 0..0,
                                content: source_info.content.clone(),
                            };

                            let key_str = key.to_string();
                            if let Some(prev_loc) = field_locs.get(&key_str) {
                                let message = format!("Field '{}' defined in multiple config files", field_path);
                                diagnostics.push(hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(hearthd_config::MergeError {
                                    field_path,
                                    message,
                                    conflicts: vec![prev_loc.clone(), conflict_loc],
                                })));
                            } else {
                                self_map.insert(key.clone(), value);
                                field_locs.insert(key_str, conflict_loc);
                            }
                        }
                    }
                })
            }
        }

        FieldType::HashMapOfStructs { .. } => {
            if field.flattened {
                Ok(quote! {
                    // Flattened field is always present (no Option wrapper)
                    for (key, partial_value) in other.#name {
                        let field_path = format!("{}.{}.{}", field_prefix, #name_str, key);

                        // Get or create the entry
                        let entry = self.#name.entry(key.clone()).or_default();

                        // Create a temporary field tracking map for this merge
                        let mut entry_field_locs = std::collections::HashMap::new();

                        // Merge the partial struct field-by-field
                        entry.merge_from(partial_value, &mut entry_field_locs, source_info, &field_path, diagnostics);
                    }
                })
            } else {
                Ok(quote! {
                    if let Some(map) = other.#name {
                        if self.#name.is_none() {
                            self.#name = Some(std::collections::HashMap::new());
                        }

                        let self_map = self.#name.as_mut().unwrap();
                        for (key, partial_value) in map {
                            let field_path = format!("{}.{}.{}", field_prefix, #name_str, key);

                            // Get or create the entry
                            let entry = self_map.entry(key.clone()).or_default();

                            // Create a temporary field tracking map for this merge
                            let mut entry_field_locs = std::collections::HashMap::new();

                            // Merge the partial struct field-by-field
                            entry.merge_from(partial_value, &mut entry_field_locs, source_info, &field_path, diagnostics);
                        }
                    }
                })
            }
        }
        FieldType::Nested(_) => {
            Ok(quote! {
                if let Some(value) = other.#name {
                    if self.#name.is_none() {
                        self.#name = Some(Default::default());
                    }

                    let entry = self.#name.as_mut().unwrap();
                    let field_path = format!("{}.{}", field_prefix, #name_str);

                    // Merge the nested partial struct field-by-field
                    entry.merge_from(value, field_locs, source_info, &field_path, diagnostics);
                }
            })
        }
    }
}

fn generate_field_merge(field: &FieldInfo, use_spans: bool) -> Result<TokenStream> {
    let name = &field.name;
    let name_str = name.to_string();

    match &field.field_type {
        FieldType::Simple(_) => {
            let loc_var = format_ident!("{}_loc", name);
            if use_spans {
                Ok(quote! {
                    if let Some(value) = config.#name {
                        let conflict_loc = value.to_conflict_location();

                        if let Some(prev_loc) = #loc_var.as_ref() {
                            diagnostics.push(hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(hearthd_config::MergeError {
                                field_path: #name_str.to_string(),
                                message: format!("Field '{}' defined in multiple config files", #name_str),
                                conflicts: vec![prev_loc.clone(), conflict_loc],
                            })));
                        } else {
                            result.#name = Some(value);
                            #loc_var = Some(conflict_loc);
                        }
                    }
                })
            } else {
                Ok(quote! {
                    if let Some(value) = config.#name {
                        let conflict_loc = hearthd_config::MergeConflictLocation {
                            file_path: source_info.file_path.clone(),
                            span: 0..0,
                            content: source_info.content.clone(),
                        };

                        if let Some(prev_loc) = #loc_var.as_ref() {
                            diagnostics.push(hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(hearthd_config::MergeError {
                                field_path: #name_str.to_string(),
                                message: format!("Field '{}' defined in multiple config files", #name_str),
                                conflicts: vec![prev_loc.clone(), conflict_loc],
                            })));
                        } else {
                            result.#name = Some(value);
                            #loc_var = Some(conflict_loc);
                        }
                    }
                })
            }
        }
        FieldType::HashMap { .. } => {
            let locs_var = format_ident!("{}_locs", name);
            if use_spans {
                Ok(quote! {
                    if let Some(map) = config.#name {
                        if result.#name.is_none() {
                            result.#name = Some(std::collections::HashMap::new());
                        }

                        let result_map = result.#name.as_mut().unwrap();
                        for (key, value_spanned) in map {
                            let conflict_loc = value_spanned.to_conflict_location();

                            let field_path = format!("{}.{}", #name_str, key);
                            if let Some(prev_loc) = #locs_var.get(&key) {
                                diagnostics.push(hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(hearthd_config::MergeError {
                                    field_path,
                                    message: format!("Map entry '{}' in '{}' defined in multiple config files", key, #name_str),
                                    conflicts: vec![prev_loc.clone(), conflict_loc],
                                })));
                            } else {
                                result_map.insert(key.clone(), value_spanned);
                                #locs_var.insert(key, conflict_loc);
                            }
                        }
                    }
                })
            } else {
                Ok(quote! {
                    if let Some(map) = config.#name {
                        if result.#name.is_none() {
                            result.#name = Some(std::collections::HashMap::new());
                        }

                        let result_map = result.#name.as_mut().unwrap();
                        for (key, value) in map {
                            let conflict_loc = hearthd_config::MergeConflictLocation {
                                file_path: source_info.file_path.clone(),
                                span: 0..0,
                                content: source_info.content.clone(),
                            };

                            let field_path = format!("{}.{}", #name_str, key);
                            if let Some(prev_loc) = #locs_var.get(&key) {
                                diagnostics.push(hearthd_config::Diagnostic::Error(hearthd_config::Error::Merge(hearthd_config::MergeError {
                                    field_path,
                                    message: format!("Map entry '{}' in '{}' defined in multiple config files", key, #name_str),
                                    conflicts: vec![prev_loc.clone(), conflict_loc],
                                })));
                            } else {
                                result_map.insert(key.clone(), value);
                                #locs_var.insert(key, conflict_loc);
                            }
                        }
                    }
                })
            }
        }
        FieldType::HashMapOfStructs { .. } => {
            let field_locs_var = format_ident!("{}_field_locs", name);
            // Generate proper field-level merging for HashMap<K, PartialStruct>
            if field.flattened {
                Ok(quote! {
                    // Flattened field is always present (no Option wrapper)
                    for (key, partial_value) in config.#name {
                        let field_prefix = format!("{}.{}", #name_str, key);

                        // Get or create the entry and its field tracking
                        let entry = result.#name.entry(key.clone()).or_default();
                        let field_locs = #field_locs_var.entry(key).or_default();

                        // Merge the partial struct field-by-field
                        entry.merge_from(partial_value, field_locs, &source_info, &field_prefix, &mut diagnostics);
                    }
                })
            } else {
                Ok(quote! {
                    if let Some(map) = config.#name {
                        if result.#name.is_none() {
                            result.#name = Some(std::collections::HashMap::new());
                        }

                        let result_map = result.#name.as_mut().unwrap();
                        for (key, partial_value) in map {
                            let field_prefix = format!("{}.{}", #name_str, key);

                            // Get or create the entry and its field tracking
                            let entry = result_map.entry(key.clone()).or_default();
                            let field_locs = #field_locs_var.entry(key).or_default();

                            // Merge the partial struct field-by-field
                            entry.merge_from(partial_value, field_locs, &source_info, &field_prefix, &mut diagnostics);
                        }
                    }
                })
            }
        }
        FieldType::Nested(_) => {
            let field_locs_var = format_ident!("{}_field_locs", name);
            Ok(quote! {
                if let Some(value) = config.#name {
                    if result.#name.is_none() {
                        result.#name = Some(Default::default());
                    }

                    let entry = result.#name.as_mut().unwrap();
                    let field_locs = #field_locs_var.entry(()).or_default();

                    // Merge the partial struct field-by-field
                    entry.merge_from(value, field_locs, &source_info, #name_str, &mut diagnostics);
                }
            })
        }
    }
}

fn generate_attach_source_impl(
    config_name: &Ident,
    fields: &[FieldInfo],
    use_spans: bool,
) -> Result<TokenStream> {
    let partial_name = format_ident!("Partial{}", config_name);

    if !use_spans {
        // If not using spans, generate an empty method (no-op)
        return Ok(quote! {
            impl #partial_name {
                /// Attach source information to all Located<T> fields in this config.
                /// This is called automatically during loading.
                /// (No-op for configs with #[config(no_span)])
                pub fn attach_source_info(&mut self, _source: hearthd_config::SourceInfo) {
                    // No-op: this config doesn't use Located<T>
                }
            }
        });
    }

    let mut attach_statements = Vec::new();

    for field in fields {
        let name = &field.name;

        match &field.field_type {
            FieldType::Simple(_) => {
                if field.flattened {
                    // Flattened fields are not Option-wrapped
                    attach_statements.push(quote! {
                        self.#name = self.#name.with_source(source.clone());
                    });
                } else {
                    attach_statements.push(quote! {
                        if let Some(ref mut value) = self.#name {
                            *value = value.clone().with_source(source.clone());
                        }
                    });
                }
            }
            FieldType::HashMap { .. } => {
                // HashMap<K, Located<V>> - attach to each value
                if field.flattened {
                    attach_statements.push(quote! {
                        for (_key, value) in self.#name.iter_mut() {
                            *value = value.clone().with_source(source.clone());
                        }
                    });
                } else {
                    attach_statements.push(quote! {
                        if let Some(ref mut map) = self.#name {
                            for (_key, value) in map.iter_mut() {
                                *value = value.clone().with_source(source.clone());
                            }
                        }
                    });
                }
            }
            FieldType::HashMapOfStructs { .. } => {
                // HashMap<K, PartialStruct> - recursively attach to each struct
                if field.flattened {
                    attach_statements.push(quote! {
                        for (_key, partial_value) in self.#name.iter_mut() {
                            partial_value.attach_source_info(source.clone());
                        }
                    });
                } else {
                    attach_statements.push(quote! {
                        if let Some(ref mut map) = self.#name {
                            for (_key, partial_value) in map.iter_mut() {
                                partial_value.attach_source_info(source.clone());
                            }
                        }
                    });
                }
            }
            FieldType::Nested(_) => {
                // Nested struct - recursively attach
                attach_statements.push(quote! {
                    if let Some(ref mut nested) = self.#name {
                        nested.attach_source_info(source.clone());
                    }
                });
            }
        }
    }

    Ok(quote! {
        impl #partial_name {
            /// Attach source information to all Located<T> fields in this config.
            /// This is called automatically during loading.
            pub fn attach_source_info(&mut self, source: hearthd_config::SourceInfo) {
                #(#attach_statements)*
            }
        }
    })
}

fn generate_load_impl(config_name: &Ident) -> Result<TokenStream> {
    let partial_name = format_ident!("Partial{}", config_name);

    Ok(quote! {
        impl #partial_name {
            pub fn from_file(path: &std::path::Path) -> Result<Self, hearthd_config::LoadError> {
                let content = std::fs::read_to_string(path).map_err(|e| hearthd_config::LoadError::Io {
                    path: path.to_path_buf(),
                    error: e.to_string(),
                })?;

                let mut config: Self = toml::from_str(&content).map_err(|e| hearthd_config::LoadError::Parse {
                    path: path.to_path_buf(),
                    error: e.to_string(),
                })?;

                let source_info = hearthd_config::SourceInfo {
                    file_path: path.to_path_buf(),
                    content,
                };

                config.source = Some(source_info.clone());
                config.attach_source_info(source_info);

                Ok(config)
            }

            pub fn load_with_imports(paths: &[std::path::PathBuf]) -> Result<Vec<Self>, hearthd_config::LoadError> {
                let mut visited = std::collections::HashSet::new();
                let mut all_configs = Vec::new();

                for path in paths {
                    Self::load_recursive(path, &mut visited, &mut all_configs)?;
                }

                Ok(all_configs)
            }

            fn load_recursive(
                path: &std::path::Path,
                visited: &mut std::collections::HashSet<std::path::PathBuf>,
                configs: &mut Vec<Self>,
            ) -> Result<(), hearthd_config::LoadError> {
                let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

                if visited.contains(&canonical_path) {
                    return Err(hearthd_config::LoadError::ImportCycle {
                        path: canonical_path.clone(),
                        cycle: visited.iter().cloned().collect(),
                    });
                }

                visited.insert(canonical_path.clone());

                let config = Self::from_file(path)?;

                for import_path in &config.imports {
                    let import_path_buf = std::path::PathBuf::from(import_path);

                    let resolved_path = if import_path_buf.is_absolute() {
                        import_path_buf
                    } else {
                        let parent_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
                        parent_dir.join(import_path_buf)
                    };

                    Self::load_recursive(&resolved_path, visited, configs)?;
                }

                configs.push(config);
                visited.remove(&canonical_path);

                Ok(())
            }
        }
    })
}

fn is_hashmap(ty: &Type) -> bool {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.last() {
            return segment.ident == "HashMap";
        }
    }
    false
}

fn extract_hashmap_types(ty: &Type) -> Result<(Type, Type)> {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.last() {
            if let PathArguments::AngleBracketed(args) = &segment.arguments {
                if args.args.len() == 2 {
                    if let (GenericArgument::Type(key), GenericArgument::Type(value)) =
                        (&args.args[0], &args.args[1])
                    {
                        return Ok((key.clone(), value.clone()));
                    }
                }
            }
        }
    }
    Err(Error::new_spanned(ty, "Expected HashMap<K, V>"))
}

fn is_option_type(ty: &Type) -> Option<Type> {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.last() {
            if segment.ident == "Option" {
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    if args.args.len() == 1 {
                        if let GenericArgument::Type(inner) = &args.args[0] {
                            return Some(inner.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

fn is_simple_type(ty: &Type) -> bool {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.last() {
            let ident = &segment.ident;
            return matches!(
                ident.to_string().as_str(),
                "bool"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "f32"
                    | "f64"
                    | "String"
                    | "str"
                    | "LogLevel" // Custom simple enum types
            );
        }
    }
    false
}

/// Parse #[config(default = "function_name")] attribute from a field
fn parse_default_fn(field: &Field) -> Option<String> {
    for attr in &field.attrs {
        if attr.path().is_ident("config") {
            if let Ok(syn::Meta::NameValue(nv)) = attr.parse_args::<syn::Meta>() {
                if nv.path.is_ident("default") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit_str),
                        ..
                    }) = &nv.value
                    {
                        return Some(lit_str.value());
                    }
                }
            }
        }
    }
    None
}

/// Generate TryFromPartial implementation for a config struct
pub fn expand_try_from_partial(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let partial_name = format_ident!("Partial{}", name);

    // Check for #[config(no_span)] attribute
    let no_span = input.attrs.iter().any(|attr| {
        if attr.path().is_ident("config") {
            if let Ok(syn::Meta::Path(path)) = attr.parse_args::<syn::Meta>() {
                return path.is_ident("no_span");
            }
        }
        false
    });
    let use_spans = !no_span;

    // Only support structs
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(Error::new_spanned(
                    name,
                    "TryFromPartial only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(Error::new_spanned(
                name,
                "TryFromPartial only supports structs",
            ));
        }
    };

    // Analyze fields (reuse existing infrastructure)
    let mut field_infos = Vec::new();
    for field in fields {
        let field_name = field.ident.as_ref().unwrap().clone();
        let field_ty = &field.ty;

        // Check if field has #[serde(flatten)] attribute
        let flattened = field.attrs.iter().any(|attr| {
            if attr.path().is_ident("serde") {
                if let Ok(syn::Meta::Path(path)) = attr.parse_args::<syn::Meta>() {
                    return path.is_ident("flatten");
                }
            }
            false
        });

        // Parse default function if present
        let default_fn = parse_default_fn(field);

        let field_type = if is_hashmap(field_ty) {
            let (key_type, value_type) = extract_hashmap_types(field_ty)?;
            if is_simple_type(&value_type) {
                FieldType::HashMap {
                    key_type,
                    value_type,
                }
            } else {
                FieldType::HashMapOfStructs {
                    key_type,
                    value_type,
                }
            }
        } else if is_simple_type(field_ty) {
            FieldType::Simple(field_ty.clone())
        } else if let Some(inner_ty) = is_option_type(field_ty) {
            if is_simple_type(&inner_ty) {
                FieldType::Simple(field_ty.clone())
            } else {
                FieldType::Nested(field_ty.clone())
            }
        } else {
            FieldType::Nested(field_ty.clone())
        };

        field_infos.push(FieldInfo {
            name: field_name,
            field_type,
            flattened,
            default_fn,
        });
    }

    // Generate field conversion code
    let field_conversions = generate_try_from_partial_conversions(&field_infos, fields, use_spans)?;

    // Collect field names for final struct construction
    let field_names: Vec<_> = field_infos.iter().map(|f| &f.name).collect();

    // Collect all HashMap key types for trait bounds
    let mut key_types: Vec<&Type> = Vec::new();
    for field in &field_infos {
        match &field.field_type {
            FieldType::HashMap { key_type, .. } | FieldType::HashMapOfStructs { key_type, .. } => {
                key_types.push(key_type);
            }
            _ => {}
        }
    }

    // Generate where clause for HashMap keys if needed
    let where_clause = if !key_types.is_empty() {
        quote! { where #(#key_types: std::fmt::Display + std::hash::Hash + Eq + Clone),* }
    } else {
        quote! {}
    };

    Ok(quote! {
        impl hearthd_config::TryFromPartial for #name #where_clause {
            fn try_from_partial(partial: #partial_name) -> Result<Self, Vec<hearthd_config::Diagnostic>> {
                let mut diagnostics = Vec::new();

                #(#field_conversions)*

                if diagnostics.is_empty() {
                    Ok(#name {
                        #(#field_names,)*
                    })
                } else {
                    Err(diagnostics)
                }
            }
        }
    })
}

fn generate_try_from_partial_conversions(
    field_infos: &[FieldInfo],
    fields: &syn::punctuated::Punctuated<Field, syn::token::Comma>,
    use_spans: bool,
) -> Result<Vec<TokenStream>> {
    let mut conversions = Vec::new();

    for (field_info, field) in field_infos.iter().zip(fields.iter()) {
        let name = &field_info.name;
        let name_str = name.to_string();
        let field_ty = &field.ty;

        let conversion = match &field_info.field_type {
            FieldType::Simple(_) => {
                // Check if the final type is Option<T> (optional field)
                let is_optional = is_option_type(field_ty).is_some();

                if is_optional {
                    // Optional field - no validation error if missing
                    if use_spans {
                        quote! {
                            let #name = partial.#name.map(|located| located.into_inner());
                        }
                    } else {
                        quote! {
                            let #name = partial.#name;
                        }
                    }
                } else if let Some(default_fn_name) = &field_info.default_fn {
                    // Required field with default function - no error, call default function
                    let default_fn_ident =
                        syn::Ident::new(default_fn_name, proc_macro2::Span::call_site());

                    if use_spans {
                        quote! {
                            let #name = partial.#name
                                .map(|located| located.into_inner())
                                .unwrap_or_else(|| #default_fn_ident());
                        }
                    } else {
                        quote! {
                            let #name = partial.#name.unwrap_or_else(|| #default_fn_ident());
                        }
                    }
                } else {
                    // Required field without default - generate validation error if missing
                    let default_value = get_default_value(field_ty);

                    if use_spans {
                        quote! {
                            let #name = if let Some(located) = partial.#name {
                                located.into_inner()
                            } else {
                                diagnostics.push(hearthd_config::Diagnostic::Error(
                                    hearthd_config::Error::Validation(hearthd_config::ValidationError {
                                        field_path: #name_str.to_string(),
                                        message: format!("{} is required", #name_str),
                                        span: None,
                                        source: partial.source.clone(),
                                    })
                                ));
                                #default_value
                            };
                        }
                    } else {
                        quote! {
                            let #name = if let Some(value) = partial.#name {
                                value
                            } else {
                                diagnostics.push(hearthd_config::Diagnostic::Error(
                                    hearthd_config::Error::Validation(hearthd_config::ValidationError {
                                        field_path: #name_str.to_string(),
                                        message: format!("{} is required", #name_str),
                                        span: None,
                                        source: partial.source.clone(),
                                    })
                                ));
                                #default_value
                            };
                        }
                    }
                }
            }
            FieldType::HashMap { .. } => {
                // HashMap of simple values - map over entries and unwrap
                if use_spans {
                    quote! {
                        let #name = partial.#name
                            .map(|hm| hm.into_iter().map(|(k, v)| (k, v.into_inner())).collect())
                            .unwrap_or_default();
                    }
                } else {
                    quote! {
                        let #name = partial.#name.unwrap_or_default();
                    }
                }
            }
            FieldType::HashMapOfStructs { value_type, .. } => {
                // HashMap of structs - iterate and recursively call try_from_partial
                if field_info.flattened {
                    quote! {
                        let mut #name = std::collections::HashMap::new();
                        for (key, partial_value) in partial.#name {
                            match <#value_type>::try_from_partial(partial_value) {
                                Ok(value) => {
                                    #name.insert(key.clone(), value);
                                }
                                Err(errs) => {
                                    diagnostics.extend(errs.into_iter().map(|d| d.prepend_path(&key)));
                                }
                            }
                        }
                    }
                } else {
                    quote! {
                        let mut #name = std::collections::HashMap::new();
                        if let Some(map) = partial.#name {
                            for (key, partial_value) in map {
                                match <#value_type>::try_from_partial(partial_value) {
                                    Ok(value) => {
                                        #name.insert(key.clone(), value);
                                    }
                                    Err(errs) => {
                                        diagnostics.extend(errs.into_iter().map(|d| d.prepend_path(&key)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            FieldType::Nested(ty) => {
                // Nested struct - recursively call try_from_partial with path prepending
                // Check if the final type is Option<T>
                let is_optional = is_option_type(field_ty).is_some();

                if is_optional {
                    // For Option<NestedStruct>, the partial is Option<PartialNestedStruct>
                    // Extract the inner type from Option
                    let inner_ty = is_option_type(ty).unwrap_or_else(|| ty.clone());

                    quote! {
                        let #name = partial.#name.map(|partial_nested| {
                            match <#inner_ty>::try_from_partial(partial_nested) {
                                Ok(value) => Some(value),
                                Err(errs) => {
                                    diagnostics.extend(errs.into_iter().map(|d| d.prepend_path(#name_str)));
                                    None
                                }
                            }
                        }).flatten();
                    }
                } else if let Some(default_fn_name) = &field_info.default_fn {
                    // Required nested field with default function - no error, call default function
                    let default_fn_ident =
                        syn::Ident::new(default_fn_name, proc_macro2::Span::call_site());

                    quote! {
                        let #name = match partial.#name.map(|partial_nested| <#ty>::try_from_partial(partial_nested)) {
                            Some(Ok(value)) => value,
                            Some(Err(errs)) => {
                                diagnostics.extend(errs.into_iter().map(|d| d.prepend_path(#name_str)));
                                #default_fn_ident()
                            }
                            None => #default_fn_ident(),
                        };
                    }
                } else {
                    // Required nested field without default - use Default::default()
                    quote! {
                        let #name = match partial.#name.map(|partial_nested| <#ty>::try_from_partial(partial_nested)) {
                            Some(Ok(value)) => value,
                            Some(Err(errs)) => {
                                diagnostics.extend(errs.into_iter().map(|d| d.prepend_path(#name_str)));
                                <#ty>::default()
                            }
                            None => <#ty>::default(),
                        };
                    }
                }
            }
        };

        conversions.push(conversion);
    }

    Ok(conversions)
}

fn get_default_value(ty: &Type) -> TokenStream {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.last() {
            let ident = &segment.ident;
            match ident.to_string().as_str() {
                "bool" => return quote! { false },
                "i8" | "i16" | "i32" | "i64" | "i128" | "u8" | "u16" | "u32" | "u64" | "u128" => {
                    return quote! { 0 };
                }
                "f32" | "f64" => return quote! { 0.0 },
                "String" => return quote! { String::new() },
                _ => {}
            }
        }
    }
    // Default to Default::default() for other types
    quote! { Default::default() }
}
