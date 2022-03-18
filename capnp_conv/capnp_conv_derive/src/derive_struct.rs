use alloc::borrow::ToOwned;

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{spanned::Spanned, FieldsNamed, Ident};

use crate::util::{
    gen_list_read_iter, gen_list_write_iter, get_vec, is_data, is_primitive, CapnpWithAttribute,
};

// TODO: Deal with the case of multiple with attributes (Should report error)
/// Get the path from a with style field attribute.
///
/// Example:
/// ```text
/// #[capnp_conv(with = Wrapper)]
/// ```
/// Will return the path `Wrapper`.
///
/// Note that in practice `Wrapper` needs to implement [ref_cast::RefCast], [From] and [Into] its
/// inner value and should have `#[repr(transparent)]`.
fn get_with_attribute(field: &syn::Field) -> Option<syn::Path> {
    for attr in &field.attrs {
        if attr.path.is_ident("capnp_conv") {
            let tts: proc_macro::TokenStream = attr.tts.to_owned().into();
            let capnp_with_attr = syn::parse::<CapnpWithAttribute>(tts).unwrap();
            return Some(capnp_with_attr.path);
        }
    }
    None
}

fn gen_type_write(field: &syn::Field, assign_defaults: impl Fn(&mut syn::Path)) -> TokenStream {
    let name = &field.ident.as_ref().unwrap();
    let init_method = syn::Ident::new(&format!("init_{}", &name), name.span());
    if let Some(with_path) = get_with_attribute(field) {
        return quote_spanned! {field.span() =>
            <#with_path as ::ref_cast::RefCast>::ref_cast(&self.#name).write_capnp(&mut writer.reborrow().#init_method());
        };
    }

    match &field.ty {
        syn::Type::Path(type_path) => {
            if type_path.qself.is_some() {
                // Self qualifier?
                unimplemented!("self qualifier");
            }

            let mut path = type_path.path.to_owned();
            assign_defaults(&mut path);

            if is_primitive(&path) {
                let set_method = syn::Ident::new(&format!("set_{}", &name), name.span());
                return quote_spanned! {field.span() =>
                    writer.reborrow().#set_method(<#path>::from(self.#name));
                };
            }

            if path.is_ident("String") || is_data(&path) {
                let set_method = syn::Ident::new(&format!("set_{}", &name), name.span());
                return quote_spanned! {field.span() =>
                    writer.reborrow().#set_method(<&#path>::from(&self.#name));
                };
            }

            if let Some(inner_path) = get_vec(&path) {
                let list_write_iter = gen_list_write_iter(&inner_path);

                // In the cases of more complicated types, list_builder needs to be mutable.
                let let_list_builder =
                    if is_primitive(&path) || path.is_ident("String") || is_data(&path) {
                        quote! { let list_builder }
                    } else {
                        quote! { let mut list_builder }
                    };

                return quote_spanned! {field.span() =>
                    {
                        #let_list_builder = {
                            writer
                            .reborrow()
                            .#init_method(self.#name.len() as u32)
                        };

                        for (index, item) in self.#name.iter().enumerate() {
                            #list_write_iter
                        }
                    }
                };
            }

            // Generic type:
            quote_spanned! {field.span() =>
                <&#path>::from(&self.#name).write_capnp(&mut writer.reborrow().#init_method());
            }
        }
        _ => unimplemented!(
            "Writer generation not implemented for type: {:?}",
            &field.ty
        ),
    }
}

fn gen_type_read(field: &syn::Field, assign_defaults: impl Fn(&mut syn::Path)) -> TokenStream {
    match &field.ty {
        syn::Type::Path(type_path) => {
            let name = &field.ident.as_ref().unwrap();
            let get_method = syn::Ident::new(&format!("get_{}", &name), name.span());

            let mut path = type_path.path.to_owned();
            assign_defaults(&mut path);

            if let Some(with_path) = get_with_attribute(field) {
                return quote_spanned! {field.span() =>
                    #name: {
                        let inner_reader = ::capnp_conv::private::CapnpResult::from(reader.#get_method()).into_result()?;
                        <::capnp_conv::private::CapnpConvWrapper<#path>>::from(<#with_path>::read_capnp(&inner_reader)?).into_wrapped_value()
                    }
                };
            }

            if type_path.qself.is_some() {
                // Self qualifier?
                unimplemented!("self qualifier");
            }

            if is_primitive(&path) {
                let get_method = syn::Ident::new(&format!("get_{}", &name), name.span());
                return quote_spanned! {field.span() =>
                    #name: reader.#get_method().into()
                };
            }

            if path.is_ident("String") || is_data(&path) {
                let get_method = syn::Ident::new(&format!("get_{}", &name), name.span());
                return quote_spanned! {field.span() =>
                    #name: reader.#get_method()?.into()
                };
            }

            if let Some(inner_path) = get_vec(&path) {
                let get_method = syn::Ident::new(&format!("get_{}", &name), name.span());
                let list_read_iter = gen_list_read_iter(&inner_path);
                return quote_spanned! {field.span() =>
                    #name: {
                        let mut res_vec = Vec::new();
                        for item_reader in reader.#get_method()? {
                            #list_read_iter
                        }
                        res_vec.into()
                    }
                };
            }

            // Generic type:
            quote_spanned! {field.span() =>
                #name: {
                    let inner_reader = ::capnp_conv::private::CapnpResult::from(reader.#get_method()).into_result()?;
                    <#path>::read_capnp(&inner_reader)?.into()
                }
            }
        }
        _ => unimplemented!(),
    }
}

pub fn gen_write_capnp_named_struct(
    fields_named: &FieldsNamed,
    rust_struct: &Ident,
    assign_defaults: impl Fn(&mut syn::Path),
) -> TokenStream {
    let recurse = fields_named
        .named
        .iter()
        .map(|field| gen_type_write(field, &assign_defaults));

    quote! {
        impl<'a> ::capnp_conv::WriteCapnp<'a> for #rust_struct {
            fn write_capnp(&self, writer: &mut <Self::Type as ::capnp::traits::Owned>::Builder) {
                #(#recurse)*
            }
        }
    }
}

pub fn gen_read_capnp_named_struct(
    fields_named: &FieldsNamed,
    rust_struct: &Ident,
    assign_defaults: impl Fn(&mut syn::Path),
) -> TokenStream {
    let recurse = fields_named
        .named
        .iter()
        .map(|field| gen_type_read(field, &assign_defaults));

    quote! {
        impl<'a> ::capnp_conv::ReadCapnp<'a> for #rust_struct {
            fn read_capnp(
                reader: &<Self::Type as ::capnp::traits::Owned<'a>>::Reader,
            ) -> Result<Self, capnp_conv::CapnpConvError> {
                Ok(#rust_struct {
                    #(#recurse,)*
                })
            }
        }
    }
}
