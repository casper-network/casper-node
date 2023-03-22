extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{DataStruct, Ident};

#[proc_macro_derive(ToBytes)]
pub fn derive_to_bytes(tokens: TokenStream) -> TokenStream {
    let ast: syn::DeriveInput = syn::parse(tokens).unwrap();

    match ast.data {
        syn::Data::Struct(st) => derive_to_bytes_for_struct(ast.ident, st),
        syn::Data::Enum(_) => panic!("enums are not supported by bytesrepr_derive"),
        syn::Data::Union(_) => panic!("unions are not supported by bytesrepr_derive"),
    }
}

#[proc_macro_derive(FromBytes)]
pub fn derive_from_bytes(tokens: TokenStream) -> TokenStream {
    let ast: syn::DeriveInput = syn::parse(tokens).unwrap();

    match ast.data {
        syn::Data::Struct(st) => derive_from_bytes_for_struct(ast.ident, st),
        syn::Data::Enum(_) => panic!("enums are not supported by bytesrepr_derive"),
        syn::Data::Union(_) => panic!("unions are not supported by bytesrepr_derive"),
    }
}

fn derive_to_bytes_for_struct(st_name: Ident, st: DataStruct) -> TokenStream {
    let mut fields_serialization = proc_macro2::TokenStream::new();
    let mut length_calculation = proc_macro2::TokenStream::new();

    // Fields encoding.
    match st.fields {
        syn::Fields::Named(ref fields) => {
            for field in &fields.named {
                let ident = field.ident.as_ref().unwrap();
                fields_serialization.extend(quote!(
                    buffer.extend(::casper_types::bytesrepr::ToBytes::to_bytes(&self.#ident)?);
                ));
                length_calculation.extend(quote!(
                    + ::casper_types::bytesrepr::ToBytes::serialized_length(&self.#ident)));
            }

            quote!(buffer.extend())
        }
        syn::Fields::Unnamed(ref fields) => {
            for idx in 0..(fields.unnamed.len()) {
                let formatted = format!("{}", idx);
                let lit = syn::LitInt::new(&formatted, st_name.span());
                fields_serialization.extend(quote!(
                    buffer.extend(::casper_types::bytesrepr::ToBytes::to_bytes(&self.#lit)?);
                ));
                length_calculation.extend(quote!(
                    + ::casper_types::bytesrepr::ToBytes::serialized_length(&self.#lit)));
            }

            quote!(buffer.extend())
        }
        // TODO: Do we (want to) support zero-sized types?
        syn::Fields::Unit => panic!("unit structs are not supported by bytesrepr_derive"),
    };

    let rv = quote! {
        impl ::casper_types::bytesrepr::ToBytes for #st_name {
            fn to_bytes(&self) -> Result<Vec<u8>, ::casper_types::bytesrepr::Error> {
                let mut buffer = ::casper_types::bytesrepr::allocate_buffer(self)?;
                #fields_serialization
                Ok(buffer)
            }

            fn serialized_length(&self) -> usize {
                0 #length_calculation
            }

            // TODO: into_bytes
            // TODO: write_bytes
        }
    };

    eprintln!("{}", rv);

    rv.into()
}

fn derive_from_bytes_for_struct(st_name: Ident, st: DataStruct) -> TokenStream {
    let mut fields_deserialization = proc_macro2::TokenStream::new();
    let mut fields_assignments = proc_macro2::TokenStream::new();

    let assignment = match st.fields {
        syn::Fields::Named(ref fields) => {
            for field in &fields.named {
                let ident = field.ident.as_ref().unwrap();
                let stack_ident = Ident::new(&format!("field_{}", ident), ident.span());
                fields_deserialization.extend(quote!(
                    let (#stack_ident, remainder) = ::casper_types::bytesrepr::FromBytes::from_bytes(remainder)?;
                ));
                fields_assignments.extend(quote!(
                    #ident: #stack_ident,
                ))
            }

            quote!(Self { #fields_assignments })
        }
        syn::Fields::Unnamed(ref fields) => {
            for idx in 0..(fields.unnamed.len()) {
                let stack_ident = Ident::new(&format!("field_{}", idx), st_name.span());
                fields_deserialization.extend(quote!(
                    let (#stack_ident, remainder) = ::casper_types::bytesrepr::FromBytes::from_bytes(remainder)?;
                ));
                fields_assignments.extend(quote!(
                   #stack_ident,
                ))
            }

            quote!(Self ( #fields_assignments ))
        }
        // TODO: Do we (want to) support zero-sized types?
        syn::Fields::Unit => panic!("unit structs are not supported by bytesrepr_derive"),
    };

    let rv = quote! {
        impl ::casper_types::bytesrepr::FromBytes for #st_name {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), ::casper_types::bytesrepr::Error> {
                let remainder = bytes;
                #fields_deserialization
                Ok((#assignment, remainder))
            }
        }
    };

    eprintln!("{}", rv);

    rv.into()
}
