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
            "Error while applying CalltableToBytes derive to {}. {}",
            struct_name, parsing_error
        ),
        Ok(token_stream) => token_stream,
    }
}

fn analyze_data(struct_name: &Ident, data: &Data) -> Result<TokenStream, ParsingError> {
    let type_description = build_type_description(data)?;

    let (serialization_length_method, serialization_method) = match type_description {
        crate::types::TypeDescription::Struct(field_definitions) => {
            if let Err(e) = validate_struct_fields(struct_name.to_string(), &field_definitions) {
                panic!("{}", e)
            }
            (
                generate_serialized_field_lengths(&field_definitions),
                generate_serialize(&field_definitions),
            )
        }
        crate::types::TypeDescription::Enum(enum_variants) => {
            if let Err(e) = validate_enum_variants(struct_name.to_string(), &enum_variants) {
                panic!("{}", e)
            }
            (
                generate_serialized_field_lengths_for_enum(struct_name, &enum_variants),
                generate_serialize_for_enum(struct_name, &enum_variants),
            )
        }
    };

    let expanded = quote! {
        impl CalltableToBytes for #struct_name  {
            #serialization_length_method
            #serialization_method
        }
        impl crate::bytesrepr::ToBytes for #struct_name {
            fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
                CalltableToBytes::serialize(self)
            }

            fn serialized_length(&self) -> usize {
                BinaryPayload::estimate_size(self.serialized_field_lengths())
            }
        }
    };

    Ok(expanded)
}

fn destructure_arg_list(field_definitions: &FieldDefinitions) -> TokenStream {
    let mut destructured = quote!();
    for definition in field_definitions.fields().iter() {
        let field_name = &definition.name_or_stub();
        destructured.extend(quote! {
            #field_name,
        });
    }
    match field_definitions {
        FieldDefinitions::Unnamed(_) => {
            quote! {
                (#destructured)
            }
        }
        FieldDefinitions::Named(_) => {
            quote! {
                {#destructured}
            }
        }
        FieldDefinitions::Unit => quote!(),
    }
}

fn generate_serialized_field_lengths_for_enum(
    enum_name: &Ident,
    variant_definitions: &BTreeMap<u8, EnumVariant>,
) -> TokenStream {
    let mut variants = quote!();
    for (_, enum_variant_definition) in variant_definitions.iter() {
        let field_definitions = &enum_variant_definition.field_definitions;
        let mut field_list = quote!();
        let variant_args = destructure_arg_list(field_definitions);

        match field_definitions {
            FieldDefinitions::Unnamed(fields) => {
                for (_position, field_definition) in fields.iter() {
                    if field_definition.index.is_some() {
                        let field_name = &field_definition.name_or_stub();
                        field_list.extend(quote! {
                            #field_name.serialized_length(),
                        });
                    }
                }
            }
            FieldDefinitions::Named(fields) => {
                for (_position, field_definition) in fields.iter() {
                    if field_definition.index.is_some() {
                        let field_name = &field_definition.name;
                        field_list.extend(quote! {
                            #field_name.serialized_length(),
                        });
                    }
                }
            }
            FieldDefinitions::Unit => {}
        }
        let variant_name = &enum_variant_definition.variant_name;
        variants.extend(quote! {
            #enum_name::#variant_name #variant_args => {
                vec![crate::bytesrepr::U8_SERIALIZED_LENGTH, #field_list]
            },
        });
    }
    quote! {
        fn serialized_field_lengths(&self) -> Vec<usize> {
            match self {
                #variants
            }
        }
    }
}

fn generate_serialize_for_enum(
    enum_name: &Ident,
    definitions: &BTreeMap<u8, EnumVariant>,
) -> TokenStream {
    let mut variants = quote!();
    for (_, definition) in definitions.iter() {
        let field_definitions = &definition.field_definitions;
        let variant_args = destructure_arg_list(field_definitions);
        let mut serialize_field_by_field = quote!();
        for field_definition in field_definitions.fields().iter() {
            if let Some(index) = &field_definition.index {
                let field_name = &field_definition.name_or_stub();
                serialize_field_by_field.extend(quote! {
                    .add_field(#index, &#field_name)?
                });
            }
        }
        let variant_index = definition.variant_index as u8;
        let variant_name = &definition.variant_name;
        variants.extend(quote! {
            #enum_name::#variant_name #variant_args => {
                crate::transaction::serialization::BinaryPayloadBuilder::new(self.serialized_field_lengths())?
                .add_field(0, &#variant_index)?
                #serialize_field_by_field
                .binary_payload_bytes()
            },
        });
    }
    quote! {
        fn serialize(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
            match self {
                #variants
            }
        }
    }
}

fn generate_serialize(definitions: &FieldDefinitions) -> TokenStream {
    let mut serialize_field_by_field = quote!();
    match definitions {
        FieldDefinitions::Unnamed(fields) => {
            for (position, definition) in fields.iter() {
                if let Some(index) = &definition.index {
                    let name = format!("{}", position);
                    serialize_field_by_field.extend(quote! {
                        .add_field(#index, &self.#name)?
                    });
                }
            }
        }
        FieldDefinitions::Named(fields) => {
            for (_position, definition) in fields.iter() {
                if let Some(index) = &definition.index {
                    let name = &definition.name;
                    serialize_field_by_field.extend(quote! {
                        .add_field(#index, &self.#name)?
                    });
                }
            }
        }
        FieldDefinitions::Unit => todo!(),
    }
    quote! {
        fn serialize(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
            crate::transaction::serialization::BinaryPayloadBuilder::new(self.serialized_field_lengths())?
            #serialize_field_by_field
            .binary_payload_bytes()
        }
    }
}

fn generate_serialized_field_lengths(definitions: &FieldDefinitions) -> TokenStream {
    let mut serialized_field_lengths = quote!();
    match definitions {
        FieldDefinitions::Unnamed(fields) => {
            for (idx, definition) in fields.iter() {
                if definition.index.is_some() {
                    let name = format!("{}", idx);
                    serialized_field_lengths.extend(quote! {
                        self.#name.serialized_length(),
                    });
                }
            }
        }
        FieldDefinitions::Named(fields) => {
            for (_, definition) in fields.iter() {
                if definition.index.is_some() {
                    let name = &definition.name;
                    serialized_field_lengths.extend(quote! {
                        self.#name.serialized_length(),
                    });
                }
            }
        }
        FieldDefinitions::Unit => {}
    }
    quote! {
        fn serialized_field_lengths(&self) -> Vec<usize> {
            vec![#serialized_field_lengths]
        }
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use syn::parse_quote;

    use crate::calltable_to_bytes::internal_derive_trait;

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
            impl CalltableToBytes for A {
                fn serialized_field_lengths(&self) -> Vec<usize> {
                    vec![
                        self.a.serialized_length(),
                        self.c.serialized_length(),
                        self.d.serialized_length(),
                    ]
                }
                fn serialize(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
                    crate::transaction::serialization::BinaryPayloadBuilder::new(self.serialized_field_lengths())?
                    .add_field(0u16, &self.a)?
                    .add_field(2u16, &self.c)?
                    .add_field(1u16, &self.d)?
                    .binary_payload_bytes()
                }
            }
            impl crate::bytesrepr::ToBytes for A {
                fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
                    CalltableToBytes::serialize(self)
                }
                fn serialized_length(&self) -> usize {
                    BinaryPayload::estimate_size(self.serialized_field_lengths())
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
            impl CalltableToBytes for A {
                fn serialized_field_lengths(&self) -> Vec<usize> {
                    match self {
                        A::A1(r#field_0, r#field_1 ,) => {
                            vec![
                                crate::bytesrepr::U8_SERIALIZED_LENGTH,
                                r#field_0.serialized_length(),
                                r#field_1.serialized_length(),
                            ]
                        },
                        A::A2 { a, b, c ,} => {
                            vec![
                                crate::bytesrepr::U8_SERIALIZED_LENGTH,
                                a.serialized_length(),
                                c.serialized_length(),
                            ]
                        },
                    }
                }
                fn serialize(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
                    match self {
                        A::A1(r#field_0, r#field_1,) => {
                            crate::transaction::serialization::BinaryPayloadBuilder::new(
                                self.serialized_field_lengths()
                            )?
                            .add_field(0, &0u8)?
                            .add_field(1u16, &r#field_0)?
                            .add_field(2u16, &r#field_1)?
                            .binary_payload_bytes()
                        },
                        A::A2 { a, b, c ,} => {crate::transaction::serialization::BinaryPayloadBuilder::new(
                            self.serialized_field_lengths()
                        )?
                        .add_field(0, &1u8)?
                        .add_field(1u16, &a)?
                        .add_field(2u16, &c)?
                        .binary_payload_bytes()},
                    }
                }
            }
            impl crate::bytesrepr::ToBytes for A {
                fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
                    CalltableToBytes::serialize(self)
                }
                fn serialized_length(&self) -> usize {
                    BinaryPayload::estimate_size(self.serialized_field_lengths())
                }
            }
        };
        assert_eq!(output_stream.to_string(), expected.to_string());
    }
}
