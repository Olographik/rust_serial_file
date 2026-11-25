use proc_macro::TokenStream;
use proc_macro2::{Literal, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, DataStruct, DeriveInput, Field, Ident};

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
