use proc_macro2::TokenStream;
use quote::quote;
use std::collections::BTreeMap;
use syn::{Data, DeriveInput, Ident};

use crate::{
    error::ParsingError,
    types::{
        build_type_description, validate_enum_variants, validate_struct_fields, EnumVariant,
        FieldDefinitions,
    },
};

pub(crate) fn internal_derive_trait(input: DeriveInput) -> TokenStream {
    let struct_name = &input.ident;
    let data: syn::Data = input.data;

    match analyze_data(struct_name, &data) {
        Err(parsing_error) => panic!(
            "Error while applying CalltableFromBytes derive to {}. {}",
            struct_name, parsing_error
        ),
        Ok(token_stream) => token_stream,
    }
}

pub(crate) fn analyze_data(struct_name: &Ident, data: &Data) -> Result<TokenStream, ParsingError> {
    let type_description = build_type_description(data)?;
    let from_bytes_method = match type_description {
        crate::types::TypeDescription::Struct(field_definitions) => {
            if let Err(e) = validate_struct_fields(struct_name.to_string(), &field_definitions) {
                panic!("{}", e)
            }
            generate_from_bytes(struct_name, &field_definitions)
        }
        crate::types::TypeDescription::Enum(enum_variants) => {
            if let Err(e) = validate_enum_variants(struct_name.to_string(), &enum_variants) {
                panic!("{}", e)
            }
            generate_from_bytes_for_variants(struct_name, &enum_variants)
        }
    };

    let expanded = quote! {
        impl CalltableFromBytes for #struct_name  {
            #from_bytes_method
        }
        impl crate::bytesrepr::FromBytes for #struct_name {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), crate::bytesrepr::Error> {
                CalltableFromBytes::from_bytes(bytes)
            }
        }
    };

    Ok(expanded)
}

fn generate_variant_code(enum_name: &Ident, variant: &EnumVariant) -> TokenStream {
    let variant_name = &variant.variant_name;
    let field_definitions = &variant.field_definitions;
    let (deserialize_fragment, fields) =
        build_deserialization_for_field_definitions(field_definitions);
    quote! {
        #deserialize_fragment
        if window.is_some() {
            return Err(bytesrepr::Error::Formatting);
        }
        Ok(#enum_name::#variant_name #fields)
    }
}
fn generate_from_bytes_for_variants(
    enum_name: &Ident,
    definitions: &BTreeMap<u8, EnumVariant>,
) -> TokenStream {
    let max_number_of_fields = (definitions.iter().fold(0, |acum, (_, definition)| {
        let cur_size = definition.field_definitions.number_of_fields();
        if cur_size > acum {
            cur_size
        } else {
            acum
        }
    }) + 1) as u32; //+1 for the tag index
    let mut enum_variants = quote!();
    for (key, variant) in definitions.iter() {
        let variant_definition = generate_variant_code(enum_name, variant);
        enum_variants.extend(quote! {
            #key => {
                #variant_definition
            },
        });
    }
    let deserialize_variant_by_variant = quote! {
        let (binary_payload, remainder) = BinaryPayload::from_bytes(#max_number_of_fields, bytes)?;
        let window = binary_payload
        .start_consuming()?
        .ok_or(bytesrepr::Error::Formatting)?;

        window.verify_index(0)?; //Tag of variant is always serialized with index 0
        let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
        let to_ret = match tag {
            #enum_variants
            _ => Err(bytesrepr::Error::Formatting),
        };
        to_ret.map(|endpoint| (endpoint, remainder))
    };

    quote! {
        fn from_bytes(bytes: &[u8]) -> Result<(#enum_name, &[u8]), crate::bytesrepr::Error> {
            #deserialize_variant_by_variant
        }
    }
}

fn generate_from_bytes(struct_name: &Ident, definitions: &FieldDefinitions) -> TokenStream {
    let (deserialize_field_by_field, new_struct_body) =
        build_deserialization_for_field_definitions(definitions);
    let max_number_of_fields = definitions.number_of_fields() as u32;
    quote! {
            fn from_bytes(bytes: &[u8]) -> Result<(#struct_name, &[u8]), crate::bytesrepr::Error> {
            let (binary_payload, remainder) = crate::transaction::serialization::BinaryPayload::from_bytes(#max_number_of_fields, bytes)?;
            let window = binary_payload
                .start_consuming()?;
            #deserialize_field_by_field
            if window.is_some() {
                return Err(bytesrepr::Error::Formatting);
            }
            let from_bytes = #struct_name #new_struct_body;
            Ok((from_bytes, remainder))
        }
    }
}

fn build_deserialization_for_field_definitions(
    definitions: &FieldDefinitions,
) -> (TokenStream, TokenStream) {
    let mut deserialize_field_by_field = quote!();
    let mut new_struct_body = quote!();
    let mut new_struct_body_inner = quote!();
    match definitions {
        FieldDefinitions::Unnamed(fields) => {
            let mut new_struct_body_inner = quote!();
            for (_idx, definition) in fields.iter() {
                let name = &definition.name_or_stub();
                let ty: &syn::Type = &definition.ty;
                if let Some(index) = definition.index {
                    deserialize_field_by_field.extend(quote! {
                        let window = window.ok_or(bytesrepr::Error::Formatting)?;
                        window.verify_index(#index)?;
                        let (#name, window) = window.deserialize_and_maybe_next::<#ty>()?;
                    });
                    new_struct_body_inner.extend(quote! {
                        #name,
                    });
                } else {
                    new_struct_body_inner.extend(quote! {
                        #ty::default(),
                    });
                }
            }
            new_struct_body = quote! {
                (#new_struct_body_inner)
            };
        }
        FieldDefinitions::Named(fields) => {
            for (_idx, definition) in fields.iter() {
                let name = &definition.name_or_stub();
                let ty: &syn::Type = &definition.ty;
                if let Some(index) = definition.index {
                    deserialize_field_by_field.extend(quote! {
                        let window = window.ok_or(bytesrepr::Error::Formatting)?;
                        window.verify_index(#index)?;
                        let (#name, window) = window.deserialize_and_maybe_next::<#ty>()?;
                    });
                    new_struct_body_inner.extend(quote! {
                        #name,
                    });
                } else {
                    new_struct_body_inner.extend(quote! {
                        #name: #ty::default(),
                    });
                }
            }
            new_struct_body = quote! {
                {#new_struct_body_inner}
            };
        }
        FieldDefinitions::Unit => {}
    }

    (deserialize_field_by_field, new_struct_body)
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use syn::parse_quote;

    use crate::calltable_from_bytes::internal_derive_trait;

    #[test]
    fn struct_derive_should_build_implementation_based_on_fields() {
        let input_derive: syn::DeriveInput = parse_quote! {
            struct A {
                #[calltable(field_index = 0)]
                a: u8,
                #[calltable(skip)]
                b: u8,
                #[calltable(field_index = 2)]
                c: u8,
                #[calltable(field_index = 1)]
                d: u8
            }
        };
        let output_stream: TokenStream = internal_derive_trait(input_derive);
        let expected: TokenStream = parse_quote! {
            impl CalltableFromBytes for A {
                fn from_bytes(bytes: &[u8]) -> Result<(A, &[u8]), crate::bytesrepr::Error> {
                    let (binary_payload, remainder) =
                        crate::transaction::serialization::BinaryPayload::from_bytes(4u32, bytes)?;
                    let window = binary_payload.start_consuming()?;
                    let window = window.ok_or(bytesrepr::Error::Formatting)?;
                    window.verify_index(0u16)?;
                    let (a, window) = window.deserialize_and_maybe_next::<u8>()?;
                    let window = window.ok_or(bytesrepr::Error::Formatting)?;
                    window.verify_index(2u16)?;
                    let (c, window) = window.deserialize_and_maybe_next::<u8>()?;
                    let window = window.ok_or(bytesrepr::Error::Formatting)?;
                    window.verify_index(1u16)?;
                    let (d, window) = window.deserialize_and_maybe_next::<u8>()?;
                    if window.is_some() {
                        return Err(bytesrepr::Error::Formatting);
                    }
                    let from_bytes = A {
                        a,
                        b: u8::default(),
                        c,
                        d,
                    };
                    Ok((from_bytes, remainder))
                }
            }
            impl crate::bytesrepr::FromBytes for A {
                fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), crate::bytesrepr::Error> {
                    CalltableFromBytes::from_bytes(bytes)
                }
            }
        };
        assert_eq!(output_stream.to_string(), expected.to_string());
    }

    #[test]
    #[should_panic]
    fn struct_derive_should_enforce_contiguous_field_indexes() {
        let input_derive: syn::DeriveInput = parse_quote! {
            struct A {
                #[calltable(field_index = 0)]
                a: u8,
                #[calltable(field_index = 2)]
                c: u8,
                #[calltable(field_index = 3)]
                d: u8
            }
        };
        internal_derive_trait(input_derive);

        let input_derive: syn::DeriveInput = parse_quote! {
            struct A {
                #[calltable(field_index = 0)]
                a: u8,
                #[calltable(field_index = 1)]
                c: u8,
                #[calltable(field_index = 5)]
                d: u8
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    #[should_panic]
    fn struct_derive_should_enforce_unique_indexes() {
        let input_derive: syn::DeriveInput = parse_quote! {
            struct A {
                #[calltable(field_index = 0)]
                a: u8,
                #[calltable(field_index = 1)]
                c: u8,
                #[calltable(field_index = 1)]
                d: u8
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    #[should_panic]
    fn struct_derive_should_enforce_indexes_starting_from_0() {
        let input_derive: syn::DeriveInput = parse_quote! {
            struct A {
                #[calltable(field_index = 3)]
                a: u8,
                #[calltable(field_index = 2)]
                c: u8,
                #[calltable(field_index = 1)]
                d: u8
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    #[should_panic]
    fn enum_derive_should_enforce_contiguous_variant_indexes() {
        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1,
                #[calltable(variant_index = 1)]
                A2,
                #[calltable(variant_index = 3)]
                A3,
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    #[should_panic]
    fn enum_derive_should_not_allow_skip() {
        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1,
                #[calltable(skip)]
                A2,
                #[calltable(variant_index = 1)]
                A3,
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    #[should_panic]
    fn enum_derive_should_enforce_unique_variant_indexes() {
        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1,
                #[calltable(variant_index = 1)]
                A2,
                #[calltable(variant_index = 1)]
                A3,
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    #[should_panic]
    fn enum_derive_should_enforce_variants_use_variant_index() {
        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1,
                #[calltable(field_index = 1)]
                A2,
                #[calltable(variant_index = 2)]
                A3,
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    #[should_panic]
    fn enum_derive_should_enforce_variant_indexes_start_from_0() {
        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 1)]
                A1,
                #[calltable(variant_index = 2)]
                A2,
                #[calltable(variant_index = 3)]
                A3,
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    #[should_panic]
    fn enum_derive_should_enforce_field_indexes_start_from_1() {
        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1{ #[calltable(field_index = 0)] a: u8, #[calltable(field_index = 1)] b: u8},
                #[calltable(variant_index = 1)]
                A2,
                #[calltable(variant_index = 2)]
                A3,
            }
        };
        internal_derive_trait(input_derive);

        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1{ #[calltable(field_index = 5)] a: u8, #[calltable(field_index = 6)] b: u8},
                #[calltable(variant_index = 1)]
                A2,
                #[calltable(variant_index = 2)]
                A3,
            }
        };
        internal_derive_trait(input_derive);

        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1( #[calltable(field_index = 0)] u8, #[calltable(field_index = 1)]  u8),
                #[calltable(variant_index = 1)]
                A2,
                #[calltable(variant_index = 2)]
                A3,
            }
        };
        internal_derive_trait(input_derive);

        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1( #[calltable(field_index = 5)] u8, #[calltable(field_index = 6)]  u8),
                #[calltable(variant_index = 1)]
                A2,
                #[calltable(variant_index = 2)]
                A3,
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    #[should_panic]
    fn enum_derive_should_enforce_field_indexes_are_contiguous() {
        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1{ #[calltable(field_index = 1)] a: u8, #[calltable(field_index = 3)] b: u8},
                #[calltable(variant_index = 1)]
                A2,
                #[calltable(variant_index = 2)]
                A3,
            }
        };
        internal_derive_trait(input_derive);

        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1( #[calltable(field_index = 1)] u8, #[calltable(field_index = 5)]  u8),
                #[calltable(variant_index = 1)]
                A2,
                #[calltable(variant_index = 2)]
                A3,
            }
        };
        internal_derive_trait(input_derive);
    }

    #[test]
    fn enum_derive_should_build_implementation_based_on_fields() {
        let input_derive: syn::DeriveInput = parse_quote! {
            enum A {
                #[calltable(variant_index = 0)]
                A1(#[calltable(field_index = 1)] u8, #[calltable(field_index = 2)] u8),
                #[calltable(variant_index = 1)]
                A2{ #[calltable(field_index = 1)] a: u8, #[calltable(skip)] b: u8, #[calltable(field_index = 2)] c: u8},
            }
        };
        let output_stream: TokenStream = internal_derive_trait(input_derive);
        let expected: TokenStream = parse_quote! {
            impl CalltableFromBytes for A {
                fn from_bytes(bytes: &[u8]) -> Result<(A, &[u8]), crate::bytesrepr::Error> {
                    let (binary_payload, remainder) = BinaryPayload::from_bytes(4u32, bytes)?;
                    let window = binary_payload
                        .start_consuming()?
                        .ok_or(bytesrepr::Error::Formatting)?;
                    window.verify_index(0)?;
                    let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
                    let to_ret = match tag {
                        0u8 => {
                            let window = window.ok_or(bytesrepr::Error::Formatting)?;
                            window.verify_index(1u16)?;
                            let (r#field_0, window) = window.deserialize_and_maybe_next::<u8>()?;
                            let window = window.ok_or(bytesrepr::Error::Formatting)?;
                            window.verify_index(2u16)?;
                            let (r#field_1, window) = window.deserialize_and_maybe_next::<u8>()?;
                            if window.is_some() {
                                return Err(bytesrepr::Error::Formatting);
                            }
                            Ok(A::A1(r#field_0, r#field_1,))
                        },
                        1u8 => {
                            let window = window.ok_or(bytesrepr::Error::Formatting)?;
                            window.verify_index(1u16)?;
                            let (a, window) = window.deserialize_and_maybe_next::<u8>()?;
                            let window = window.ok_or(bytesrepr::Error::Formatting)?;
                            window.verify_index(2u16)?;
                            let (c, window) = window.deserialize_and_maybe_next::<u8>()?;
                            if window.is_some() {
                                return Err(bytesrepr::Error::Formatting);
                            }
                            Ok(A::A2 {
                                a,
                                b: u8::default(),
                                c,
                            })
                        },
                        _ => Err(bytesrepr::Error::Formatting),
                    };
                    to_ret.map(|endpoint| (endpoint, remainder))
                }
            }
            impl crate::bytesrepr::FromBytes for A {
                fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), crate::bytesrepr::Error> {
                    CalltableFromBytes::from_bytes(bytes)
                }
            }
        };
        assert_eq!(output_stream.to_string(), expected.to_string());
    }
}
