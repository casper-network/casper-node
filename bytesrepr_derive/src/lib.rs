extern crate proc_macro;

use std::env;

use once_cell::sync::Lazy;
use proc_macro::TokenStream;
use quote::quote;
use syn::{DataEnum, DataStruct, Generics, Ident, LitInt};

static DEBUG: Lazy<bool> = Lazy::new(|| {
    if let Some(val) = env::var("BYTESREPR_DERIVE_DEBUG").ok() {
        val != "0"
    } else {
        false
    }
});

/// Top-level proc macro for `#[derive(ToBytes)]`.
#[proc_macro_derive(ToBytes)]
pub fn derive_to_bytes(tokens: TokenStream) -> TokenStream {
    let ast: syn::DeriveInput = syn::parse(tokens).unwrap();

    let output = match ast.data {
        syn::Data::Struct(st) => derive_to_bytes_for_struct(ast.ident, st, ast.generics),
        syn::Data::Enum(en) => derive_to_bytes_for_enum(ast.ident, en, ast.generics),
        syn::Data::Union(_) => panic!("unions are not supported by bytesrepr_derive"),
    };

    if *DEBUG {
        eprintln!("{}", output);
    }

    output
}

/// Top-level proc macro for `#[derive(FromBytes)]`.
#[proc_macro_derive(FromBytes)]
pub fn derive_from_bytes(tokens: TokenStream) -> TokenStream {
    let ast: syn::DeriveInput = syn::parse(tokens).unwrap();

    let output = match ast.data {
        syn::Data::Struct(st) => derive_from_bytes_for_struct(ast.ident, st, ast.generics),
        syn::Data::Enum(en) => derive_from_bytes_for_enum(ast.ident, en, ast.generics),
        syn::Data::Union(_) => panic!("unions are not supported by bytesrepr_derive"),
    };

    if *DEBUG {
        eprintln!("{}", output);
    }

    output
}

/// Given a set of generics, returns a vec of all idents.
fn param_idents_from_generics(generics: &Generics) -> Vec<Ident> {
    generics
        .params
        .iter()
        .map(|p| match p {
            syn::GenericParam::Lifetime(_) => {
                panic!("lifetime generics are currently not supported by bytesrepr_derive")
            }
            syn::GenericParam::Type(ty) => ty.ident.clone(),
            syn::GenericParam::Const(_) => {
                panic!("const generics are currently not supported by bytesrepr_derive")
            }
        })
        .collect()
}

fn generate_where_clause(
    generics: &Generics,
    extra_req: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let mut where_clause = match &generics.where_clause {
        Some(w) => {
            quote!(#w)
        }
        None => {
            quote!(where)
        }
    };
    for ident in param_idents_from_generics(generics) {
        where_clause.extend(quote!(#ident: #extra_req,))
    }
    where_clause
}

fn generate_generic_args(generics: &Generics) -> proc_macro2::TokenStream {
    let idents = param_idents_from_generics(&generics);
    if !idents.is_empty() {
        quote!(<#(#idents),*>)
    } else {
        Default::default()
    }
}

/// Proc macro for `#[derive(ToBytes)]` for `struct`s.
fn derive_to_bytes_for_struct(st_name: Ident, st: DataStruct, generics: Generics) -> TokenStream {
    let mut fields_serialization = proc_macro2::TokenStream::new();
    let mut owned_fields_serialization = proc_macro2::TokenStream::new();
    let mut length_calculation = proc_macro2::TokenStream::new();

    let generic_idents = generate_generic_args(&generics);
    let where_clause = generate_where_clause(&generics, quote!(::casper_types::bytesrepr::ToBytes));

    match st.fields {
        syn::Fields::Named(ref fields) => {
            for (idx, field) in fields.named.iter().enumerate() {
                let ident = field.ident.as_ref().unwrap();
                fields_serialization.extend(quote!(
                    ::casper_types::bytesrepr::ToBytes::write_bytes(&self.#ident, writer)?;
                ));

                // A little hack is required here to have optimal code; basically we do not want to
                // construct an additional vec if we can just pass through the inner one unchanged.
                if fields.named.len() != 1 {
                    if idx == 0 {
                        owned_fields_serialization.extend(quote!(
                            let mut buffer = Vec::new();
                        ));
                    }
                    owned_fields_serialization.extend(quote!(
                        buffer.extend(::casper_types::bytesrepr::ToBytes::into_bytes(self.#ident)?);
                    ));
                } else {
                    owned_fields_serialization.extend(quote!(
                        let buffer = ::casper_types::bytesrepr::ToBytes::into_bytes(self.#ident)?;
                    ));
                }

                length_calculation.extend(quote!(
                    + ::casper_types::bytesrepr::ToBytes::serialized_length(&self.#ident)));
            }
        }
        syn::Fields::Unnamed(ref fields) => {
            for idx in 0..(fields.unnamed.len()) {
                let formatted = format!("{}", idx);
                let lit = LitInt::new(&formatted, st_name.span());

                if fields.unnamed.len() != 1 {
                    if idx == 0 {
                        owned_fields_serialization.extend(quote!(
                            let mut buffer = Vec::new();
                        ));
                    }
                    owned_fields_serialization.extend(quote!(
                        buffer.extend(::casper_types::bytesrepr::ToBytes::into_bytes(self.#lit)?);
                    ));
                } else {
                    owned_fields_serialization.extend(quote!(
                        let buffer = ::casper_types::bytesrepr::ToBytes::into_bytes(self.0)?;
                    ));
                }

                fields_serialization.extend(quote!(
                    ::casper_types::bytesrepr::ToBytes::write_bytes(&self.#lit, writer)?;
                ));
                length_calculation.extend(quote!(
                    + ::casper_types::bytesrepr::ToBytes::serialized_length(&self.#lit)));
            }
        }
        syn::Fields::Unit => {
            // Nothing to do, just ensure we return an empty buffer later on.
            owned_fields_serialization.extend(quote!(
                let buffer = Vec::new();
            ));
        }
    };

    quote! {
        impl #generic_idents ::casper_types::bytesrepr::ToBytes for #st_name #generics #where_clause {
            #[inline]
            fn to_bytes(&self) -> Result<Vec<u8>, ::casper_types::bytesrepr::Error> {
                let mut buffer = Vec::new();
                self.write_bytes(&mut buffer)?;
                Ok(buffer)
            }

            #[inline]
            fn serialized_length(&self) -> usize {
                0 #length_calculation
            }

            #[inline]
            fn into_bytes(self) -> Result<Vec<u8>, ::casper_types::bytesrepr::Error>
            where
                Self: Sized,
            {
                #owned_fields_serialization
                return Ok(buffer)
            }


            #[inline]

            fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), ::casper_types::bytesrepr::Error> {
                ::casper_types::bytesrepr::reserve_buffer(writer, &self)?;
                #fields_serialization
                Ok(())
            }
        }
    }.into()
}

/// Proc macro for `#[derive(FromBytes)]` for `struct`s.
fn derive_from_bytes_for_struct(st_name: Ident, st: DataStruct, generics: Generics) -> TokenStream {
    let mut fields_deserialization = proc_macro2::TokenStream::new();
    let mut fields_assignments = proc_macro2::TokenStream::new();

    let generic_idents = generate_generic_args(&generics);
    let where_clause =
        generate_where_clause(&generics, quote!(::casper_types::bytesrepr::FromBytes));

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
        syn::Fields::Unit => {
            // Nothing to do, unit structs deserialize from anything.
            quote!(Self)
        }
    };

    quote! {
        impl #generic_idents ::casper_types::bytesrepr::FromBytes for #st_name #generics #where_clause {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), ::casper_types::bytesrepr::Error> {
                let remainder = bytes;
                #fields_deserialization
                Ok((#assignment, remainder))
            }
        }
    }.into()
}

/// Proc macro for `#[derive(ToBytes)]` for `enum`s.
fn derive_to_bytes_for_enum(en_name: Ident, en: DataEnum, generics: Generics) -> TokenStream {
    let mut write_bytes_variant_match = proc_macro2::TokenStream::new();
    let mut length_variant_match = proc_macro2::TokenStream::new();

    let generic_idents = generate_generic_args(&generics);
    let where_clause = generate_where_clause(&generics, quote!(::casper_types::bytesrepr::ToBytes));

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
                        writer.push(#discriminator);
                ));
                for field in &fields.named {
                    let ident = field.ident.as_ref().unwrap();
                    fields_serialization.extend(quote!(
                        ::casper_types::bytesrepr::ToBytes::write_bytes(#ident, writer)?;
                    ));
                    length_calculation.extend(quote!(
                        + ::casper_types::bytesrepr::ToBytes::serialized_length(#ident)));
                }

                let field_names: Vec<_> = fields.named.iter().map(|f| f.ident.clone()).collect();
                write_bytes_variant_match.extend(quote! {
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
                        writer.push(#discriminator);
                ));

                let mut field_names = Vec::new();
                for idx in 0..(fields.unnamed.len()) {
                    let idx_ident = Ident::new(&format!("field_{}", idx), en_name.span());
                    fields_serialization.extend(quote!(
                        ::casper_types::bytesrepr::ToBytes::write_bytes(#idx_ident, writer)?;
                    ));
                    length_calculation.extend(quote!(
                        + ::casper_types::bytesrepr::ToBytes::serialized_length(#idx_ident)));

                    field_names.push(idx_ident);
                }

                write_bytes_variant_match.extend(quote! {
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
            syn::Fields::Unit => {
                fields_serialization.extend(quote!(
                        writer.push(#discriminator);
                ));

                write_bytes_variant_match.extend(quote! {
                    Self::#variant_ident => {
                        #fields_serialization
                    }
                });

                length_variant_match.extend(quote! {
                    Self::#variant_ident => {
                        0
                    }
                });
            }
        }
    }

    quote!(
        impl #generic_idents ::casper_types::bytesrepr::ToBytes for #en_name #generics #where_clause {
            #[inline]
            fn to_bytes(&self) -> Result<Vec<u8>, ::casper_types::bytesrepr::Error> {
                let mut buffer = Vec::new();
                self.write_bytes(&mut buffer)?;
                Ok(buffer)
            }

             #[inline]
            fn serialized_length(&self) -> usize {
                1 + match self {
                    #length_variant_match
                }
            }

            // There is nothing to gain from implementing `into_bytes` for enums, as we always have
            // to prefix with the discriminant, causing a memcopy/move.

            #[inline]
            fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), ::casper_types::bytesrepr::Error> {
                ::casper_types::bytesrepr::reserve_buffer(writer, &self)?;
                match self {
                    #write_bytes_variant_match
                }
                Ok(())
            }
        }
    ).into()
}

/// Proc macro for `#[derive(FromBytes)]` for `enum`s.
fn derive_from_bytes_for_enum(en_name: Ident, en: DataEnum, generics: Generics) -> TokenStream {
    let mut from_bytes_variant_match = proc_macro2::TokenStream::new();

    let generic_idents = generate_generic_args(&generics);
    let where_clause =
        generate_where_clause(&generics, quote!(::casper_types::bytesrepr::FromBytes));

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
                        Ok((Self::#variant_ident ( #(#field_names),* ), remainder))
                    }
                });
            }
            syn::Fields::Unit => {
                from_bytes_variant_match.extend(quote! {
                    #discriminator => {
                        Ok((Self::#variant_ident, remainder))
                    }
                });
            }
        }
    }

    quote!(
        impl #generic_idents ::casper_types::bytesrepr::FromBytes for #en_name #generics #where_clause {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), ::casper_types::bytesrepr::Error> {
                let (tag, remainder) = u8::from_bytes(bytes)?;
                match tag {
                    #from_bytes_variant_match
                    _ => Err(::casper_types::bytesrepr::Error::Formatting)
                }
            }
        }
    ).into()
}
