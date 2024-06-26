pub mod approval;
pub mod initiator_addr;
pub mod pricing_mode;
pub mod transaction_entry_point;
pub mod transaction_invocation_target;
pub mod transaction_scheduling;
pub mod transaction_target;
pub mod transaction_v1;
pub mod transaction_v1_body;
pub mod transaction_v1_header;
use alloc::{collections::BTreeMap, vec::Vec};
pub use transaction_v1::{TransactionV1BinaryFromBytes, TransactionV1BinaryToBytes};
pub use transaction_v1_body::{
    deserialize_transaction_body, serialize_transaction_body, transaction_body_serialized_length,
};
pub use transaction_v1_header::{TransactionV1HeaderFromBytes, TransactionV1HeaderToBytes};

use crate::bytesrepr::{
    Bytes, Error, FromBytes, ToBytes, U16_SERIALIZED_LENGTH, U32_SERIALIZED_LENGTH,
    U8_SERIALIZED_LENGTH,
};
pub const TRANSACTION_V1_SERIALIZATION_VERSION: u8 = 1;
const EMPTY_BTREEMAP_OVERHEAD: usize = 4; // The number of bytes that a BTreeMap will add
pub fn tag_only_serialized_length() -> usize {
    serialized_length_for_field_sizes(vec![U8_SERIALIZED_LENGTH])
}

pub fn map_stored_field_size(field_size: usize) -> usize {
    U16_SERIALIZED_LENGTH /* field index size */ + U32_SERIALIZED_LENGTH /* variable which stores length of bytes*/ + field_size
}

///Calculates the serialized length of the binary representation of a map containing indexed fields
pub fn serialized_length_for_field_sizes(field_sizes: Vec<usize>) -> usize {
    let mut output_size: usize = EMPTY_BTREEMAP_OVERHEAD;
    for field_size in field_sizes {
        output_size += map_stored_field_size(field_size);
    }
    output_size
}

pub fn consume_field<T: FromBytes>(
    data_map: &mut BTreeMap<u16, Bytes>,
    field_index: u16,
) -> Result<T, Error> {
    data_map
        .remove(&field_index)
        .map(|bytes| {
            T::from_bytes(&bytes).and_then(|(value, remainder)| {
                if !remainder.is_empty() {
                    Err(Error::Formatting)
                } else {
                    Ok(value)
                }
            })
        })
        .unwrap_or_else(|| Err(Error::Formatting))
}

pub fn deserialize_fields_map(bytes: &[u8]) -> Result<(BTreeMap<u16, Bytes>, &[u8]), Error> {
    BTreeMap::<u16, Bytes>::from_bytes(bytes)
}

pub fn serialize_fields_map(fields: BTreeMap<u16, Bytes>) -> Result<Vec<u8>, Error> {
    fields.to_bytes()
}
