//! Procedural macros for MemioTauri.
//!
//! This crate provides the `#[derive(MemioModel)]` macro which
//! implements all necessary traits for serialization.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Lit, Type, parse_macro_input};

/// Derives all necessary traits for serialization with Rkyv.
///
/// This macro automatically implements:
/// - `rkyv::Archive`
/// - `rkyv::Serialize`
/// - `rkyv::Deserialize`
/// - `bytecheck::CheckBytes`
///
/// # Example
/// ```ignore
/// use memio_core::MemioModel;
///
/// #[derive(MemioModel)]
/// pub struct Player {
///     pub name: String,
///     pub score: u64,
///     pub position: (f32, f32),
/// }
/// ```
#[proc_macro_derive(MemioModel)]
pub fn derive_memio_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let mut fields_tokens = Vec::new();
    let mut errors = Vec::new();

    if let Data::Struct(data) = &input.data
        && let Fields::Named(named) = &data.fields
    {
        for field in &named.named {
            let field_ident = field.ident.as_ref().unwrap();
            let field_name = field_ident.to_string();
            let field_ty = match field_type_token(&field.ty) {
                Ok(ty) => ty,
                Err(err) => {
                    errors.push(err);
                    continue;
                }
            };
            fields_tokens.push(quote! {
                ::memio_core::MemioField {
                    name: #field_name,
                    offset: ::std::mem::offset_of!(rkyv::Archived<#name #ty_generics>, #field_ident),
                    ty: #field_ty,
                }
            });
        }
    }

    let errors_tokens: Vec<_> = errors
        .into_iter()
        .map(|err| quote! { compile_error!(#err); })
        .collect();

    let expanded = quote! {
        #(#errors_tokens)*

        impl #impl_generics ::memio_core::MemioSchema for #name #ty_generics #where_clause {
            fn schema() -> &'static [::memio_core::MemioField] {
                static FIELDS: &[::memio_core::MemioField] = &[
                    #(#fields_tokens),*
                ];
                FIELDS
            }
        }
    };

    TokenStream::from(expanded)
}

/// Helper attribute for marking fields that should use custom serialization.
#[proc_macro_attribute]
pub fn memio_skip(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Pass through unchanged - this is a marker attribute
    item
}

fn field_type_token(ty: &Type) -> Result<proc_macro2::TokenStream, String> {
    match ty {
        Type::Path(path) => {
            let ident = path.path.segments.last().map(|s| s.ident.to_string());
            match ident.as_deref() {
                Some("u8") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::U8) },
                ),
                Some("u16") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::U16) },
                ),
                Some("u32") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::U32) },
                ),
                Some("u64") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::U64) },
                ),
                Some("i8") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::I8) },
                ),
                Some("i16") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::I16) },
                ),
                Some("i32") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::I32) },
                ),
                Some("i64") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::I64) },
                ),
                Some("f32") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::F32) },
                ),
                Some("f64") => Ok(
                    quote! { ::memio_core::MemioFieldType::Scalar(::memio_core::MemioScalarType::F64) },
                ),
                Some(other) => Err(format!("MemioModel: unsupported field type `{}`", other)),
                None => Err("MemioModel: unsupported field type".to_string()),
            }
        }
        Type::Array(array) => {
            let elem = scalar_type_token(&array.elem)?;
            let len = match &array.len {
                Expr::Lit(expr) => match &expr.lit {
                    Lit::Int(int_lit) => int_lit.base10_parse::<usize>().ok(),
                    _ => None,
                },
                _ => None,
            }
            .ok_or_else(|| "MemioModel: array length must be a literal integer".to_string())?;

            Ok(quote! {
                ::memio_core::MemioFieldType::Array { elem: #elem, len: #len }
            })
        }
        _ => Err("MemioModel: unsupported field type".to_string()),
    }
}

fn scalar_type_token(ty: &Type) -> Result<proc_macro2::TokenStream, String> {
    let ident = match ty {
        Type::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    };

    match ident.as_deref() {
        Some("u8") => Ok(quote! { ::memio_core::MemioScalarType::U8 }),
        Some("u16") => Ok(quote! { ::memio_core::MemioScalarType::U16 }),
        Some("u32") => Ok(quote! { ::memio_core::MemioScalarType::U32 }),
        Some("u64") => Ok(quote! { ::memio_core::MemioScalarType::U64 }),
        Some("i8") => Ok(quote! { ::memio_core::MemioScalarType::I8 }),
        Some("i16") => Ok(quote! { ::memio_core::MemioScalarType::I16 }),
        Some("i32") => Ok(quote! { ::memio_core::MemioScalarType::I32 }),
        Some("i64") => Ok(quote! { ::memio_core::MemioScalarType::I64 }),
        Some("f32") => Ok(quote! { ::memio_core::MemioScalarType::F32 }),
        Some("f64") => Ok(quote! { ::memio_core::MemioScalarType::F64 }),
        Some(other) => Err(format!("MemioModel: unsupported field type `{}`", other)),
        None => Err("MemioModel: unsupported field type".to_string()),
    }
}
