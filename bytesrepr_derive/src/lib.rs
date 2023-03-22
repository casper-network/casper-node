extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{DataEnum, DataStruct, Ident, LitInt};

/// Top-level proc macro for `#[derive(ToBytes)]`.
#[proc_macro_derive(ToBytes)]
pub fn derive_to_bytes(tokens: TokenStream) -> TokenStream {
    let ast: syn::DeriveInput = syn::parse(tokens).unwrap();

    match ast.data {
        syn::Data::Struct(st) => derive_to_bytes_for_struct(ast.ident, st),
        syn::Data::Enum(en) => derive_to_bytes_for_enum(ast.ident, en),
        syn::Data::Union(_) => panic!("unions are not supported by bytesrepr_derive"),
    }
}

/// Top-level proc macro for `#[derive(FromBytes)]`.
#[proc_macro_derive(FromBytes)]
pub fn derive_from_bytes(tokens: TokenStream) -> TokenStream {
    let ast: syn::DeriveInput = syn::parse(tokens).unwrap();

    match ast.data {
        syn::Data::Struct(st) => derive_from_bytes_for_struct(ast.ident, st),
        syn::Data::Enum(en) => derive_from_bytes_for_enum(ast.ident, en),
        syn::Data::Union(_) => panic!("unions are not supported by bytesrepr_derive"),
    }
}

/// Proc macro for `#[derive(ToBytes)]` for `struct`s.
fn derive_to_bytes_for_struct(st_name: Ident, st: DataStruct) -> TokenStream {
    let mut fields_serialization = proc_macro2::TokenStream::new();
    let mut length_calculation = proc_macro2::TokenStream::new();

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
        }
        syn::Fields::Unnamed(ref fields) => {
            for idx in 0..(fields.unnamed.len()) {
                let formatted = format!("{}", idx);
                let lit = LitInt::new(&formatted, st_name.span());
                fields_serialization.extend(quote!(
                    buffer.extend(::casper_types::bytesrepr::ToBytes::to_bytes(&self.#lit)?);
                ));
                length_calculation.extend(quote!(
                    + ::casper_types::bytesrepr::ToBytes::serialized_length(&self.#lit)));
            }
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

/// Proc macro for `#[derive(FromBytes)]` for `struct`s.
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

/// Proc macro for `#[derive(ToBytes)]` for `struct`s.
fn derive_to_bytes_for_enum(en_name: Ident, en: DataEnum) -> TokenStream {
    let mut to_bytes_variant_match = proc_macro2::TokenStream::new();
    let mut length_variant_match = proc_macro2::TokenStream::new();

    for (idx, variant) in en.variants.iter().enumerate() {
        assert!(
            idx <= u8::MAX as usize,
            "cannot serialize this many variants"
        );

        if variant.discriminant.is_some() {
            // TODO: Decide whether we want to change the encoding based on these variant
            //       assignments or not. For now, its best to not do either.
            panic!("explicit discriminants are currently not supported by bytesrepr_derive");
        }

        let mut fields_serialization = proc_macro2::TokenStream::new();
        let mut length_calculation = proc_macro2::TokenStream::new();

        let discriminator = LitInt::new(&format!("{}u8", idx), variant.ident.span());

        let variant_ident = &variant.ident;
        match variant.fields {
            syn::Fields::Named(ref fields) => {
                fields_serialization.extend(quote!(
                        buffer.push(#discriminator);
                ));
                for field in &fields.named {
                    let ident = field.ident.as_ref().unwrap();
                    fields_serialization.extend(quote!(
                        buffer.extend(::casper_types::bytesrepr::ToBytes::to_bytes(#ident)?);
                    ));
                    length_calculation.extend(quote!(
                        + ::casper_types::bytesrepr::ToBytes::serialized_length(#ident)));
                }

                let field_names: Vec<_> = fields.named.iter().map(|f| f.ident.clone()).collect();
                to_bytes_variant_match.extend(quote! {
                    Self::#variant_ident { #(#field_names),* } => {
                        #fields_serialization
                    }
                });

                length_variant_match.extend(quote! {
                    Self::#variant_ident { #(#field_names),* } => {
                        0 #length_calculation
                    }
                });
            }
            syn::Fields::Unnamed(ref fields) => {
                fields_serialization.extend(quote!(
                        buffer.push(#discriminator);
                ));

                let mut field_names = Vec::new();
                for idx in 0..(fields.unnamed.len()) {
                    let idx_ident = Ident::new(&format!("field_{}", idx), en_name.span());
                    fields_serialization.extend(quote!(
                        buffer.extend(::casper_types::bytesrepr::ToBytes::to_bytes(#idx_ident)?);
                    ));
                    length_calculation.extend(quote!(
                        + ::casper_types::bytesrepr::ToBytes::serialized_length(#idx_ident)));

                    field_names.push(idx_ident);
                }

                to_bytes_variant_match.extend(quote! {
                    Self::#variant_ident ( #(#field_names),* ) => {
                        #fields_serialization
                    }
                });

                length_variant_match.extend(quote! {
                    Self::#variant_ident ( #(#field_names),* ) => {
                        0 #length_calculation
                    }
                });
            }
            // TODO: Do we (want to) support zero-sized types?
            syn::Fields::Unit => {
                panic!("unit enum variants are not supported by bytesrepr_derive")
            }
        }
    }

    let rv = quote!(
        impl ::casper_types::bytesrepr::ToBytes for #en_name {
            fn to_bytes(&self) -> Result<Vec<u8>, ::casper_types::bytesrepr::Error> {
                let mut buffer = ::casper_types::bytesrepr::allocate_buffer(self)?;
                match self {
                    #to_bytes_variant_match
                }
                Ok(buffer)
            }

            fn serialized_length(&self) -> usize {
                1 + match self {
                    #length_variant_match
                }
            }

            // TODO: into_bytes
            // TODO: write_bytes
        }
    );
    eprintln!("{}", rv);

    rv.into()
}

/// Proc macro for `#[derive(ToBytes)]` for `struct`s.
fn derive_from_bytes_for_enum(en_name: Ident, en: DataEnum) -> TokenStream {
    let mut from_bytes_variant_match = proc_macro2::TokenStream::new();

    for (idx, variant) in en.variants.iter().enumerate() {
        assert!(
            idx <= u8::MAX as usize,
            "cannot deserialize this many variants"
        );

        if variant.discriminant.is_some() {
            // TODO: Decide whether we want to change the encoding based on these variant
            //       assignments or not. For now, its best to not do either.
            panic!("explicit discriminants are currently not supported by bytesrepr_derive");
        }

        let mut field_deserializations = proc_macro2::TokenStream::new();
        let variant_ident = &variant.ident;
        let discriminator = LitInt::new(&format!("{}u8", idx), variant.ident.span());

        match variant.fields {
            syn::Fields::Named(ref fields) => {
                let mut field_assignments = proc_macro2::TokenStream::new();

                for field in &fields.named {
                    let ident = field.ident.as_ref().unwrap();
                    let stack_ident = Ident::new(&format!("field_{}", ident), ident.span());
                    field_deserializations.extend(quote!(
                        let (#stack_ident, remainder) = ::casper_types::bytesrepr::FromBytes::from_bytes(remainder)?;
                    ));
                    field_assignments.extend(quote!(
                        #ident: #stack_ident,
                    ))
                }

                // Note: The `Formatting` error as a catch-all is a weirdness with precedent.
                from_bytes_variant_match.extend(quote! {
                    #discriminator => {
                        #field_deserializations
                        Ok((Self::#variant_ident { #field_assignments }, remainder))
                    }
                });
            }
            syn::Fields::Unnamed(ref fields) => {
                let mut field_names = Vec::new();
                for idx in 0..(fields.unnamed.len()) {
                    let field_ident = Ident::new(&format!("field_{}", idx), en_name.span());
                    field_deserializations.extend(quote!(
                        let (#field_ident, remainder) = ::casper_types::bytesrepr::FromBytes::from_bytes(remainder)?;
                    ));
                    field_names.push(field_ident);
                }

                from_bytes_variant_match.extend(quote! {
                    #discriminator => {
                        #field_deserializations
                        Ok((Self::#variant_ident ( #(#field_names,)* ), remainder))
                    }
                });
            }
            syn::Fields::Unit => {
                panic!("unit enum variants are not supported by bytesrepr_derive")
            }
        }
    }

    let rv = quote!(
        impl ::casper_types::bytesrepr::FromBytes for #en_name {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), ::casper_types::bytesrepr::Error> {
                let (tag, remainder) = u8::from_bytes(bytes)?;
                match tag {
                    #from_bytes_variant_match
                    _ => Err(::casper_types::bytesrepr::Error::Formatting)
                }
            }
        }
    );
    eprintln!("{}", rv);

    rv.into()
}
