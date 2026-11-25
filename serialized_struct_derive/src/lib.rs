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

#[proc_macro_derive(EncodeRaw)]
pub fn derive_encode_raw(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    if let Some(errors) = reject_pointer_sized_fields(&input, "EncodeRaw") {
        return errors.into();
    }
    derive_encode_raw_impl(&input).into()
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
#[proc_macro_derive(DecodeRaw)]
pub fn derive_decode_raw(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    if let Some(errors) = reject_pointer_sized_fields(&input, "DecodeRaw") {
        return errors.into();
    }
    derive_decode_raw_impl(&input).into()
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
#[proc_macro_derive(SerializedStruct)]
pub fn derive_serialized_struct(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    if let Some(errors) = reject_pointer_sized_fields(&input, "SerializedStruct") {
        return errors.into();
    }
    let name = &input.ident;

    let encode_impl = derive_encode_raw_impl(&input);
    let decode_impl = derive_decode_raw_impl(&input);

    let field_inits = match input.data {
        syn::Data::Struct(ref s) => match &s.fields {
            syn::Fields::Named(ref f) => f.named.iter().enumerate().map(derive_serialized_field),
            syn::Fields::Unnamed(ref f) => {
                f.unnamed.iter().enumerate().map(derive_serialized_field)
            }
            syn::Fields::Unit => {
                return derive_serialized_wrap(name, quote! {0}, &encode_impl, &decode_impl).into();
            }
        },
        _ => panic!("#[derive(EncodeRaw)] supports only structs"),
    };

    derive_serialized_wrap(name, quote! {#(#field_inits)*}, &encode_impl, &decode_impl).into()
}

#[cfg(test)]
mod tests {
    use super::{pointer_sized_name, reject_pointer_sized_fields};

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
}
