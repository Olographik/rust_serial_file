use proc_macro::TokenStream;
use proc_macro2::{Literal, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, DataStruct, DeriveInput, Field, Ident, Type};

/// Returns `"usize"` or `"isize"` when `ty` is written as one of these
/// pointer-sized primitives (`usize`, `core::primitive::usize`,
/// `::std::primitive::isize`, ...).
///
/// The check is syntactic: a type alias of `usize` is not recognized here,
/// but it still fails to compile because `usize` does not implement the
/// traits.
fn pointer_sized_name(ty: &Type) -> Option<&'static str> {
    let path = match ty {
        Type::Group(group) => return pointer_sized_name(&group.elem),
        Type::Paren(paren) => return pointer_sized_name(&paren.elem),
        Type::Path(type_path) if type_path.qself.is_none() => &type_path.path,
        _ => return None,
    };
    if path
        .segments
        .iter()
        .any(|segment| !segment.arguments.is_none())
    {
        return None;
    }
    let names: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let primitive = match names.as_slice() {
        [name] if path.leading_colon.is_none() => name,
        [root, module, name] if (root == "core" || root == "std") && module == "primitive" => name,
        _ => return None,
    };
    match primitive.as_str() {
        "usize" => Some("usize"),
        "isize" => Some("isize"),
        _ => None,
    }
}

/// Builds one compile error per `usize`/`isize` field of a struct, or `None`
/// when there is none.
///
/// The encoded layout must not depend on the target, and these types are
/// 4 bytes on 32-bit targets (such as `wasm32`) and 8 bytes on 64-bit ones.
fn reject_pointer_sized_fields(input: &DeriveInput, derive_name: &str) -> Option<TokenStream2> {
    let syn::Data::Struct(ref data) = input.data else {
        return None;
    };
    let errors: Vec<TokenStream2> = data
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let primitive = pointer_sized_name(&field.ty)?;
            let field_name = match field.ident {
                Some(ref ident) => ident.to_string(),
                None => index.to_string(),
            };
            let message = format!(
                "#[derive({derive_name})] does not support the `{primitive}` field `{field_name}` \
                 of `{}`: its size depends on the target platform. Use a fixed-width type \
                 (for example `u64` or `i64`), or implement `EncodeRaw`, `DecodeRaw` and \
                 `SerializedStruct` by hand for this struct.",
                input.ident
            );
            Some(syn::Error::new_spanned(&field.ty, message).to_compile_error())
        })
        .collect();
    if errors.is_empty() {
        None
    } else {
        Some(quote! { #(#errors)* })
    }
}

fn derive_encode_field(index: (usize, &Field)) -> TokenStream2 {
    let name = match index.1.ident {
        Some(ref d) => quote! {#d},
        None => {
            let l = Literal::usize_unsuffixed(index.0);
            quote! {#l}
        }
    };

    quote! {
        offset += self.#name.encode(&mut data[offset..]);
    }
}

fn derive_encode_struct_wrap(name: &Ident, internal: impl Into<TokenStream2>) -> TokenStream2 {
    let internal = internal.into();
    quote! {
        impl serialized_struct::EncodeRaw for #name {
            fn encode(&self, data: &mut [u8]) -> usize{
                #internal
            }
        }
    }
}
fn derive_encode_struct_impl(name: &Ident, s: &DataStruct) -> TokenStream2 {
    // We only handle structs with named fields
    let field_inits = match &s.fields {
        syn::Fields::Named(ref f) => f.named.iter().enumerate().map(derive_encode_field),
        syn::Fields::Unnamed(ref f) => f.unnamed.iter().enumerate().map(derive_encode_field),
        syn::Fields::Unit => {
            return derive_encode_struct_wrap(name, quote! {0});
        }
    };

    derive_encode_struct_wrap(
        name,
        quote! {
            let mut offset = 0;

            #(#field_inits)*

            offset
        },
    )
}

fn derive_encode_raw_impl(input: &DeriveInput) -> TokenStream2 {
    let name = &input.ident;

    // We only handle structs with named fields
    match input.data {
        syn::Data::Struct(ref s) => derive_encode_struct_impl(name, s),
        // cannot evaluated the SIZE at compile time to make decision, need custom implementation
        // syn::Data::Union(ref s) => derive_encode_union_impl(name, s),
        _ => panic!("#[derive(EncodeRaw)] supports only structs"),
    }
}

/// Full `EncodeRaw` expansion: compile errors for pointer-sized fields, or
/// the implementation.
fn expand_encode_raw(input: &DeriveInput) -> TokenStream2 {
    reject_pointer_sized_fields(input, "EncodeRaw").unwrap_or_else(|| derive_encode_raw_impl(input))
}

#[proc_macro_derive(EncodeRaw)]
pub fn derive_encode_raw(input: TokenStream) -> TokenStream {
    expand_encode_raw(&parse_macro_input!(input as DeriveInput)).into()
}

fn derive_decode_field(field: (usize, &Field)) -> TokenStream2 {
    let name = match field.1.ident {
        Some(ref d) => quote! {#d},
        None => {
            let l = Literal::usize_unsuffixed(field.0);
            quote! {#l}
        }
    };
    let ty = &field.1.ty;
    quote! {
        #name: <#ty as serialized_struct::DecodeRaw>::from_le_bytes_slices(bytes, offset)
    }
}

fn derive_decode_raw_wrap(name: &Ident, internal: impl Into<TokenStream2>) -> TokenStream2 {
    let internal = internal.into();
    quote! {
        impl serialized_struct::DecodeRaw for #name {
            fn from_le_bytes_slices(bytes: &[u8], offset:&mut usize) -> Self {
                #internal
            }
        }
    }
}
fn derive_decode_raw_impl(input: &DeriveInput) -> TokenStream2 {
    let name = &input.ident;

    let field_inits = match input.data {
        syn::Data::Struct(ref s) => match &s.fields {
            syn::Fields::Named(ref f) => f.named.iter().enumerate().map(derive_decode_field),
            syn::Fields::Unnamed(ref f) => f.unnamed.iter().enumerate().map(derive_decode_field),
            syn::Fields::Unit => {
                return derive_decode_raw_wrap(name, quote! {Self{}});
            }
        },
        _ => panic!("#[derive(DecodeRaw)] supports only structs"),
    };

    derive_decode_raw_wrap(
        name,
        quote! {
                    Self {
                        #(#field_inits),*
                    }
        },
    )
}
/// Full `DecodeRaw` expansion: compile errors for pointer-sized fields, or
/// the implementation.
fn expand_decode_raw(input: &DeriveInput) -> TokenStream2 {
    reject_pointer_sized_fields(input, "DecodeRaw").unwrap_or_else(|| derive_decode_raw_impl(input))
}

#[proc_macro_derive(DecodeRaw)]
pub fn derive_decode_raw(input: TokenStream) -> TokenStream {
    expand_decode_raw(&parse_macro_input!(input as DeriveInput)).into()
}

fn derive_serialized_field(field: (usize, &Field)) -> TokenStream2 {
    // let name = field.ident.as_ref().unwrap();
    let ty = &field.1.ty;
    let field_expr = quote! {
        <#ty as serialized_struct::SerializedStruct>::SIZE
    };
    if field.0 > 0 {
        quote! { + #field_expr }
    } else {
        field_expr
    }
}

fn derive_serialized_wrap(
    name: &Ident,
    internal: impl Into<TokenStream2>,
    encode_impl: &TokenStream2,
    decode_impl: &TokenStream2,
) -> TokenStream2 {
    let internal = internal.into();
    quote! {
        #encode_impl

        #decode_impl

        impl serialized_struct::SerializedStruct for #name {
            const SIZE:usize = #internal;
        }
    }
}
/// Full `SerializedStruct` expansion (also implementing `EncodeRaw` and
/// `DecodeRaw`): compile errors for pointer-sized fields, or the
/// implementations.
fn expand_serialized_struct(input: &DeriveInput) -> TokenStream2 {
    if let Some(errors) = reject_pointer_sized_fields(input, "SerializedStruct") {
        return errors;
    }
    let name = &input.ident;

    let encode_impl = derive_encode_raw_impl(input);
    let decode_impl = derive_decode_raw_impl(input);

    let field_inits = match input.data {
        syn::Data::Struct(ref s) => match &s.fields {
            syn::Fields::Named(ref f) => f.named.iter().enumerate().map(derive_serialized_field),
            syn::Fields::Unnamed(ref f) => {
                f.unnamed.iter().enumerate().map(derive_serialized_field)
            }
            syn::Fields::Unit => {
                return derive_serialized_wrap(name, quote! {0}, &encode_impl, &decode_impl);
            }
        },
        _ => panic!("#[derive(EncodeRaw)] supports only structs"),
    };

    derive_serialized_wrap(name, quote! {#(#field_inits)*}, &encode_impl, &decode_impl)
}

#[proc_macro_derive(SerializedStruct)]
pub fn derive_serialized_struct(input: TokenStream) -> TokenStream {
    expand_serialized_struct(&parse_macro_input!(input as DeriveInput)).into()
}

#[cfg(test)]
mod tests {
    use super::{
        expand_decode_raw, expand_encode_raw, expand_serialized_struct, pointer_sized_name,
        reject_pointer_sized_fields,
    };

    fn parse(input: &str) -> syn::DeriveInput {
        syn::parse_str(input).unwrap()
    }

    /// Token strings have spaces between tokens; remove them to compare.
    fn compact(tokens: proc_macro2::TokenStream) -> String {
        tokens.to_string().replace(' ', "")
    }

    fn name_of(ty: &str) -> Option<&'static str> {
        pointer_sized_name(&syn::parse_str(ty).unwrap())
    }

    #[test]
    fn detects_pointer_sized_types() {
        assert_eq!(name_of("usize"), Some("usize"));
        assert_eq!(name_of("isize"), Some("isize"));
        assert_eq!(name_of("core::primitive::usize"), Some("usize"));
        assert_eq!(name_of("::std::primitive::isize"), Some("isize"));
        assert_eq!(name_of("(usize)"), Some("usize"));
        // Types coming from `macro_rules!` expansions can be wrapped in an
        // invisible group.
        let grouped = syn::Type::Group(syn::TypeGroup {
            group_token: Default::default(),
            elem: Box::new(syn::parse_str("isize").unwrap()),
        });
        assert_eq!(pointer_sized_name(&grouped), Some("isize"));

        assert_eq!(name_of("u64"), None);
        assert_eq!(name_of("i32"), None);
        assert_eq!(name_of("Option<usize>"), None);
        assert_eq!(name_of("[usize; 2]"), None);
        assert_eq!(name_of("Index"), None);
        assert_eq!(name_of("::usize"), None);
        assert_eq!(name_of("other::primitive::usize"), None);
        assert_eq!(name_of("core::usize"), None);
        assert_eq!(name_of("<T as Trait>::usize"), None);
    }

    #[test]
    fn rejects_only_structs_with_pointer_sized_fields() {
        let named: syn::DeriveInput =
            syn::parse_str("struct A { a: u8, b: usize, c: isize }").unwrap();
        let errors = reject_pointer_sized_fields(&named, "SerializedStruct")
            .unwrap()
            .to_string();
        assert_eq!(errors.matches("compile_error").count(), 2);
        assert!(errors.contains("`usize` field `b` of `A`"));
        assert!(errors.contains("`isize` field `c` of `A`"));

        let tuple: syn::DeriveInput = syn::parse_str("struct B(u32, usize);").unwrap();
        let errors = reject_pointer_sized_fields(&tuple, "EncodeRaw")
            .unwrap()
            .to_string();
        assert!(errors.contains("#[derive(EncodeRaw)]"));
        assert!(errors.contains("`usize` field `1` of `B`"));

        for accepted in ["struct C { a: u64 }", "struct D;", "enum E { X }"] {
            let input: syn::DeriveInput = syn::parse_str(accepted).unwrap();
            assert!(reject_pointer_sized_fields(&input, "DecodeRaw").is_none());
        }
    }

    #[test]
    fn expands_named_tuple_and_unit_structs() {
        let named = compact(expand_serialized_struct(&parse(
            "struct A { x: u8, y: u32 }",
        )));
        assert!(named.contains("implserialized_struct::EncodeRawforA"));
        assert!(named.contains("offset+=self.x.encode(&mutdata[offset..]);"));
        assert!(named
            .contains("x:<u8asserialized_struct::DecodeRaw>::from_le_bytes_slices(bytes,offset)"));
        assert!(named.contains(
            "constSIZE:usize=<u8asserialized_struct::SerializedStruct>::SIZE+<u32asserialized_struct::SerializedStruct>::SIZE;"
        ));

        let tuple = compact(expand_serialized_struct(&parse("struct B(u16, i64);")));
        assert!(tuple.contains("self.1.encode"));
        assert!(tuple.contains("1:<i64asserialized_struct::DecodeRaw>"));

        let unit = compact(expand_serialized_struct(&parse("struct C;")));
        assert!(unit.contains("constSIZE:usize=0;"));
        assert!(unit.contains("Self{}"));

        let encode = compact(expand_encode_raw(&parse("struct D { a: f32 }")));
        assert!(encode.contains("implserialized_struct::EncodeRawforD"));
        assert!(!encode.contains("DecodeRaw"));
        let unit_encode = compact(expand_encode_raw(&parse("struct E;")));
        assert!(unit_encode.contains("->usize{0}"));

        let decode = compact(expand_decode_raw(&parse("struct F(u8);")));
        assert!(decode.contains("implserialized_struct::DecodeRawforF"));
        assert!(!decode.contains("EncodeRaw"));
    }

    #[test]
    fn expansions_report_pointer_sized_fields() {
        for expanded in [
            expand_serialized_struct(&parse("struct A { a: usize }")),
            expand_encode_raw(&parse("struct A { a: usize }")),
            expand_decode_raw(&parse("struct A { a: usize }")),
        ] {
            let text = compact(expanded);
            assert!(text.contains("compile_error"));
            assert!(!text.contains("implserialized_struct"));
        }
    }

    #[test]
    #[should_panic(expected = "supports only structs")]
    fn serialized_struct_rejects_enums() {
        expand_serialized_struct(&parse("enum A { X }"));
    }

    #[test]
    #[should_panic(expected = "#[derive(EncodeRaw)] supports only structs")]
    fn encode_raw_rejects_enums() {
        expand_encode_raw(&parse("enum A { X }"));
    }

    #[test]
    #[should_panic(expected = "#[derive(DecodeRaw)] supports only structs")]
    fn decode_raw_rejects_unions() {
        expand_decode_raw(&parse("union A { x: u8 }"));
    }
}
