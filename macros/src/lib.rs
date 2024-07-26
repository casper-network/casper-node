mod calltable_from_bytes;
mod calltable_to_bytes;
mod error;
mod types;
use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

pub(crate) const CALLTABLE_ATTRIBUTE: &str = "calltable";
pub(crate) const FIELD_INDEX_ATTRIBUTE: &str = "field_index";
pub(crate) const VARIANT_INDEX_ATTRIBUTE: &str = "variant_index";

/// A `derive` type macro to auto-add calltable-like serialization to an enum or structure. It also
/// provides an automatic implementation of `ToBytes`. # For structs:
///     You need to mark it's fields with #[calltable(field_index = X)] attributes. The `X` denotes
/// at what order the field will be stored. If a field shuldn't be serialized, you should use
/// #[calltable(skip)]. For example:
///     ```
///     #[derive(CalltableToBytes)]
///     pub struct A {
///        #[calltable(field_index = 0)]
///        pub(crate) field1: u8,
///        #[calltable(field_index = 1)]
///        pub(crate) field2: u8,
///     }
///     ```
///    
///     will produce:
///     ```
///     impl CalltableToBytes for A {
///         fn serialized_field_lengths(&self) -> Vec<usize> {
///             vec![
///                 self.field1.serialized_length(),
///                 self.field2.serialized_length(),
///             ]
///         }
///         fn serialize(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
///             crate::transaction::serialization::BinaryPayloadBuilder::new(
///                 self.serialized_field_lengths(),
///             )?
///             .add_field(0u16, &self.field1)?
///             .add_field(1u16, &self.field2)?
///             .binary_payload_bytes()
///         }
///     }
///     impl crate::bytesrepr::ToBytes for A {
///         fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
///             CalltableToBytes::serialize(self)
///         }
///         fn serialized_length(&self) -> usize {
///             BinaryPayload::estimate_size(self.serialized_field_lengths())
///         }
///     }
///     ```
///
/// # For enums:
///     Each enum variant needs to be marked with #[calltable(variant_index = X)] so that we can
/// store a tag which discerns variants. Other than that each field of each variant needs to have
/// #[calltable(field_index = Y)], as for structs. The only difference is that Y is indexed from
/// `1`, not from `0` (index `0` is reserved for storing the tag). For example:
///     An enum definition like:
///     ```
///         #[derive(CalltableToBytes)]
///         enum A {
///             #[calltable(variant_index = 0)]
///             A1(
///                 #[calltable(field_index = 1)] u8,
///                 #[calltable(field_index = 2)] u8,
///             ),
///             #[calltable(variant_index = 1)]
///             A2 {
///                 #[calltable(field_index = 1)]
///                 a: u8,
///                 #[calltable(skip)]
///                 b: u8,
///                 #[calltable(field_index = 2)]
///                 c: u8,
///             },
///         }
///     ```
///     Will produce code:
///     ```
///        impl CalltableToBytes for A {
///            fn serialized_field_lengths(&self) -> Vec<usize> {
///                match self {
///                    A::A1(r#field_0, r#field_1) => {
///                        vec![
///                            crate::bytesrepr::U8_SERIALIZED_LENGTH,
///                            r#field_0.serialized_length(),
///                            r#field_1.serialized_length(),
///                        ]
///                    }
///                    A::A2 { a, b, c } => {
///                        vec![
///                            crate::bytesrepr::U8_SERIALIZED_LENGTH,
///                            a.serialized_length(),
///                            c.serialized_length(),
///                        ]
///                    }
///                }
///            }
///            fn serialize(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
///                match self {
///                    A::A1(r#field_0, r#field_1) => {
///                        crate::transaction::serialization::BinaryPayloadBuilder::new(
///                            self.serialized_field_lengths(),
///                        )?
///                        .add_field(0, &0u8)?
///                        .add_field(1u16, &r#field_0)?
///                        .add_field(2u16, &r#field_1)?
///                        .binary_payload_bytes()
///                    }
///                    A::A2 { a, b, c } =>
///                        crate::transaction::serialization::BinaryPayloadBuilder::new(
///                        self.serialized_field_lengths())?
///                    .add_field(0, &1u8)?
///                    .add_field(1u16, &a)?
///                    .add_field(2u16, &c)?
///                    .binary_payload_bytes(),
///                }
///            }
///        }
///        impl crate::bytesrepr::ToBytes for A {
///            fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
///                CalltableToBytes::serialize(self)
///            }
///            fn serialized_length(&self) -> usize {
///                BinaryPayload::estimate_size(self.serialized_field_lengths())
///            }
///        }
///     ```
#[proc_macro_derive(CalltableToBytes, attributes(calltable))]
pub fn generate_to_bytes_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    proc_macro::TokenStream::from(calltable_to_bytes::internal_derive_trait(input))
}

/// A `derive` type macro to auto-add calltable-like deserialization to an enum or structure. It
/// also provides an automatic implementation of `FromBytes`.
/// # For structs:
///    You need to mark it's fields with #[calltable(field_index = X)] attributes. The `X` denotes
/// at what order the field will be stored. If a field shuldn't be serialized, you should use
/// #[calltable(skip)]
///     For example:
///     ```
///     #[derive(CalltableFromBytes)]
///     pub struct A {
///        #[calltable(field_index = 0)]
///        pub(crate) field1: u8,
///        #[calltable(field_index = 1)]
///        pub(crate) field2: u8,
///     }
///     ```
///    
///     will produce:
///     ```
///         impl CalltableFromBytes for A {
///             fn from_bytes(bytes: &[u8]) -> Result<(A, &[u8]), crate::bytesrepr::Error> {
///                 let (binary_payload, remainder) =
///                     crate::transaction::serialization::BinaryPayload::from_bytes(2u32, bytes)?;
///                 let window = binary_payload.start_consuming()?;
///                 let window = window.ok_or(bytesrepr::Error::Formatting)?;
///                 window.verify_index(0u16)?;
///                 let (field1, window) = window.deserialize_and_maybe_next::<u8>()?;
///                 let window = window.ok_or(bytesrepr::Error::Formatting)?;
///                 window.verify_index(1u16)?;
///                 let (field2, window) = window.deserialize_and_maybe_next::<u8>()?;
///                 if window.is_some() {
///                     return Err(bytesrepr::Error::Formatting);
///                 }
///                 let from_bytes = A { field1, field2 };
///                 Ok((from_bytes, remainder))
///             }
///         }
///         impl crate::bytesrepr::FromBytes for A {
///             fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), crate::bytesrepr::Error> {
///                 CalltableFromBytes::from_bytes(bytes)
///             }
///         }
///     ```
///
/// # For enums:
///    Each enum variant needs to be marked with #[calltable(variant_index = X)] so that we can
/// store a tag which discerns variants. Other than that each field of each variant needs to have
/// #[calltable(field_index = Y)], as for structs. The only difference is that Y is indexed from
/// `1`, not from `0` (index `0` is reserved for storing the tag).
///    For example:
///    An enum definition like:
///    ```
///        #[derive(CalltableFromBytes)]
///        enum A {
///            #[calltable(variant_index = 0)]
///            A1(
///                #[calltable(field_index = 1)] u8,
///                #[calltable(field_index = 2)] u8,
///            ),
///            #[calltable(variant_index = 1)]
///            A2 {
///                #[calltable(field_index = 1)]
///                a: u8,
///                #[calltable(skip)]
///                b: u8,
///                #[calltable(field_index = 2)]
///                c: u8,
///            },
///        }
///    ```
///    Will produce code:
///    ```
///       impl CalltableFromBytes for A {
///           fn from_bytes(bytes: &[u8]) -> Result<(A, &[u8]), crate::bytesrepr::Error> {
///               let (binary_payload, remainder) = BinaryPayload::from_bytes(4u32, bytes)?;
///               let window = binary_payload
///                   .start_consuming()?
///                   .ok_or(bytesrepr::Error::Formatting)?;
///               window.verify_index(0)?;
///               let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
///               let to_ret = match tag {
///                   0u8 => {
///                       let window = window.ok_or(bytesrepr::Error::Formatting)?;
///                       window.verify_index(1u16)?;
///                       let (r#field_0, window) = window.deserialize_and_maybe_next::<u8>()?;
///                       let window = window.ok_or(bytesrepr::Error::Formatting)?;
///                       window.verify_index(2u16)?;
///                       let (r#field_1, window) = window.deserialize_and_maybe_next::<u8>()?;
///                       if window.is_some() {
///                           return Err(bytesrepr::Error::Formatting);
///                       }
///                       Ok(A::A1(r#field_0, r#field_1))
///                   }
///                   1u8 => {
///                       let window = window.ok_or(bytesrepr::Error::Formatting)?;
///                       window.verify_index(1u16)?;
///                       let (a, window) = window.deserialize_and_maybe_next::<u8>()?;
///                       let window = window.ok_or(bytesrepr::Error::Formatting)?;
///                       window.verify_index(2u16)?;
///                       let (c, window) = window.deserialize_and_maybe_next::<u8>()?;
///                       if window.is_some() {
///                           return Err(bytesrepr::Error::Formatting);
///                       }
///                       Ok(A::A2 {
///                           a,
///                           b: u8::default(),
///                           c,
///                       })
///                   }
///                   _ => Err(bytesrepr::Error::Formatting),
///               };
///               to_ret.map(|endpoint| (endpoint, remainder))
///           }
///       }
///       impl crate::bytesrepr::FromBytes for A {
///           fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), crate::bytesrepr::Error> {
///               CalltableFromBytes::from_bytes(bytes)
///           }
///       }
///    ```
#[proc_macro_derive(CalltableFromBytes, attributes(calltable))]
pub fn generate_from_bytes_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    proc_macro::TokenStream::from(calltable_from_bytes::internal_derive_trait(input))
}
