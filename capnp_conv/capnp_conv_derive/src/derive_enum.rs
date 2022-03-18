use alloc::{borrow::ToOwned, string::ToString};

use proc_macro2::TokenStream;
use quote::quote;
use syn::{spanned::Spanned, DataEnum, Fields, Ident, Variant};

use heck::SnakeCase;

use crate::util::{
    gen_list_read_iter, gen_list_write_iter, get_vec, is_data, is_primitive, CapnpWithAttribute,
};

// TODO: Deal with the case of multiple with attributes (Should report error)
/// Get the path from a with style field attribute.
/// Example:
/// ```text
/// #[capnp_conv(with = Wrapper<u128>)]
/// ```
/// Will return the path `Wrapper<u128>`
fn get_with_attribute(variant: &syn::Variant) -> Option<syn::Path> {
    for attr in &variant.attrs {
        if attr.path.is_ident("capnp_conv") {
            let tts: proc_macro::TokenStream = attr.tts.to_owned().into();
            let capnp_with_attr = syn::parse::<CapnpWithAttribute>(tts).unwrap();
            return Some(capnp_with_attr.path);
        }
    }
    None
}

fn gen_type_write(variant: &Variant, assign_defaults: impl Fn(&mut syn::Path)) -> TokenStream {
    let opt_with_path = get_with_attribute(variant);
    let variant_name = &variant.ident;
    let variant_snake_name = variant_name.to_string().to_snake_case();
    let init_method = syn::Ident::new(&format!("init_{}", &variant_snake_name), variant.span());

    if let Some(with_path) = opt_with_path {
        return quote! {
            #variant_name(x) => <#with_path as ::ref_cast::RefCast>::ref_cast(x).write_capnp(&mut writer.reborrow().#init_method()),
        };
    }

    match &variant.fields {
        Fields::Unnamed(fields_unnamed) => {
            let unnamed = &fields_unnamed.unnamed;
            if unnamed.len() != 1 {
                unimplemented!("gen_type_write: Amount of unnamed fields is not 1!");
            }

            let pair = unnamed.last().unwrap();
            let last_ident = match pair {
                syn::punctuated::Pair::End(last_ident) => last_ident,
                _ => unreachable!(),
            };

            let mut path = match &last_ident.ty {
                syn::Type::Path(type_path) => type_path.path.to_owned(),
                _ => {
                    unimplemented!(
                        "Could not create write_capnp expression for type in enum: {:?}",
                        &last_ident.ty
                    );
                }
            };
            assign_defaults(&mut path);

            if is_primitive(&path) {
                let set_method =
                    syn::Ident::new(&format!("set_{}", &variant_snake_name), variant.span());
                return quote! {
                    #variant_name(x) => writer.#set_method(*<&#path>::from(x)),
                };
            }

            if is_data(&path) {
                let set_method =
                    syn::Ident::new(&format!("set_{}", &variant_snake_name), variant.span());
                return quote! {
                    #variant_name(x) => writer.#set_method(<&#path>::from(x)),
                };
            }

            if path.is_ident("String") {
                let set_method =
                    syn::Ident::new(&format!("set_{}", &variant_snake_name), variant.span());
                return quote! {
                    #variant_name(x) => writer.#set_method(x),
                };
            }

            // The case of list:
            if let Some(inner_path) = get_vec(&path) {
                let init_method =
                    syn::Ident::new(&format!("init_{}", &variant_snake_name), variant.span());
                let list_write_iter = gen_list_write_iter(&inner_path);

                // In the cases of more complicated types, list_builder needs to be mutable.
                let let_list_builder =
                    if is_primitive(&path) || path.is_ident("String") || is_data(&path) {
                        quote! { let list_builder }
                    } else {
                        quote! { let mut list_builder }
                    };

                return quote! {
                    #variant_name(vec) => {
                        #let_list_builder = writer
                            .reborrow()
                            .#init_method(vec.len() as u32);

                        for (index, item) in vec.iter().enumerate() {
                            #list_write_iter
                        }
                    },
                };
            }

            quote! {
                #variant_name(x) => <&#path>::from(x).write_capnp(&mut writer.reborrow().#init_method()),
            }
        }

        Fields::Unit => {
            let set_method =
                syn::Ident::new(&format!("set_{}", &variant_snake_name), variant.span());
            quote! {
                #variant_name => writer.#set_method(()),
            }
        }
        Fields::Named(_) => unimplemented!(),
    }
}

pub fn gen_write_capnp_enum(
    data_enum: &DataEnum,
    rust_enum: &Ident,
    assign_defaults: impl Fn(&mut syn::Path),
) -> TokenStream {
    let recurse = data_enum.variants.iter().map(|variant| {
        let type_write = gen_type_write(variant, &assign_defaults);
        quote! {
            #rust_enum::#type_write
        }
    });

    quote! {
        impl<'a> capnp_conv::WriteCapnp<'a> for #rust_enum {
            fn write_capnp(&self, writer: &mut <Self::Type as capnp::traits::Owned>::Builder) {
                match &self {
                    #(#recurse)*
                };
            }
        }
    }
}

fn gen_type_read(
    variant: &Variant,
    rust_enum: &Ident,
    assign_defaults: impl Fn(&mut syn::Path),
) -> TokenStream {
    let variant_name = &variant.ident;

    match &variant.fields {
        Fields::Unnamed(fields_unnamed) => {
            let unnamed = &fields_unnamed.unnamed;
            if unnamed.len() != 1 {
                unimplemented!("gen_type_read: Amount of unnamed fields is not 1!");
            }

            let pair = unnamed.last().unwrap();
            let last_ident = match pair {
                syn::punctuated::Pair::End(last_ident) => last_ident,
                _ => unreachable!(),
            };

            let mut path = match &last_ident.ty {
                syn::Type::Path(type_path) => type_path.path.to_owned(),
                _ => {
                    panic!("{:?}", get_with_attribute(variant));
                }
            };

            assign_defaults(&mut path);

            if let Some(with_path) = get_with_attribute(variant) {
                return quote! {
                    #variant_name(variant_reader) => {
                        let variant_reader = ::capnp_conv::private::CapnpResult::from(variant_reader).into_result()?;
                        let value = <::capnp_conv::private::CapnpConvWrapper<#path>>::from(<#with_path>::read_capnp(&variant_reader)?).into_wrapped_value();
                        #rust_enum::#variant_name(value)
                    },
                };
            }

            if is_primitive(&path) {
                return quote! {
                    #variant_name(x) => #rust_enum::#variant_name(x.into()),
                };
            }

            if is_data(&path) || path.is_ident("String") {
                return quote! {
                    #variant_name(x) => #rust_enum::#variant_name(x?.into()),
                };
            }

            if let Some(inner_path) = get_vec(&path) {
                // The case of a list:
                let list_read_iter = gen_list_read_iter(&inner_path);
                return quote! {
                    #variant_name(list_reader) => {
                        let mut res_vec = Vec::new();
                        for item_reader in list_reader? {
                            #list_read_iter
                        }
                        #rust_enum::#variant_name(res_vec)
                    }
                };
            }

            quote! {
                #variant_name(variant_reader) => {
                    let variant_reader = ::capnp_conv::private::CapnpResult::from(variant_reader).into_result()?;
                    #rust_enum::#variant_name(<#path>::read_capnp(&variant_reader)?.into())
                },
            }
        }

        Fields::Unit => {
            quote! {
                #variant_name(()) => #rust_enum::#variant_name,
            }
        }
        // Rust enum variants don't have named fields (?)
        Fields::Named(_) => unreachable!(),
    }
}

pub fn gen_read_capnp_enum(
    data_enum: &DataEnum,
    rust_enum: &Ident,
    capnp_struct: &syn::Path,
    assign_defaults: impl Fn(&mut syn::Path),
) -> TokenStream {
    let recurse = data_enum.variants.iter().map(|variant| {
        let type_read = gen_type_read(variant, rust_enum, &assign_defaults);
        quote! {
            #capnp_struct::#type_read
        }
    });

    quote! {
        impl<'a> ::capnp_conv::ReadCapnp<'a> for #rust_enum {
            fn read_capnp(
                reader: &<Self::Type as ::capnp::traits::Owned>::Reader,
            ) -> Result<Self, ::capnp_conv::CapnpConvError> {
                Ok(match reader.which()? {
                    #(#recurse)*
                })
            }
        }
    }
}
