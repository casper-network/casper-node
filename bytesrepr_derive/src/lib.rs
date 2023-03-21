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
                ))
            }

            quote!(buffer.extend())
        }
        syn::Fields::Unnamed(ref fields) => {
            for idx in 0..(fields.unnamed.len()) {
                fields_serialization.extend(quote!(
                    buffer.extend(::casper_types::bytesrepr::ToBytes::to_bytes(&self.#idx)?);
                ))
            }

            quote!(buffer.extend())
        }
        // TODO: Do we (want to) support zero-sized types?
        syn::Fields::Unit => panic!("unit structs are not supported by bytesrepr_derive"),
    };

    // Serialization length calculation.
    match st.fields {
        syn::Fields::Named(fields) => {
            for field in fields.named {
                let ident = field.ident.as_ref().unwrap();
                length_calculation.extend(quote!(
                    + ::casper_types::bytesrepr::ToBytes::serialized_length(&self.#ident)
                ))
            }

            quote!(buffer.extend())
        }
        syn::Fields::Unnamed(ref fields) => {
            for idx in 0..(fields.unnamed.len()) {
                length_calculation.extend(quote!(
                    + ::casper_types::bytesrepr::ToBytes::serialized_length(&self.#idx)
                ))
            }

            quote!(buffer.extend())
        }
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

            // fn into_bytes(self) -> Result<Vec<u8>, ::casper_types::bytesrepr::Error>
            // where Self: Sized {
            //     todo!("macro implementation incomplete")
            // }
        }
    };

    eprintln!("{}", rv);

    rv.into()
}
