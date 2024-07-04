use crate::{
    bytesrepr::{self, Bytes, ToBytes, U8_SERIALIZED_LENGTH},
    TransactionInvocationTarget, TransactionRuntime, TransactionTarget,
};
use alloc::{collections::BTreeMap, vec::Vec};

use super::{
    consume_field, deserialize_fields_map, serialize_fields_map, serialized_length_for_field_sizes,
};

pub const NATIVE_TAG: u8 = 0;
pub const STORED_TAG: u8 = 1;
pub const SESSION_TAG: u8 = 2;

const TAG_FIELD_META_INDEX: u16 = 0;
const ID_FIELD_META_INDEX: u16 = 1;
const RUNTIME_FIELD_META_INDEX: u16 = 2;
const MODULE_BYTES_FIELD_META_INDEX: u16 = 3;

pub fn native_serialized_length() -> usize {
    serialized_length_for_field_sizes(vec![U8_SERIALIZED_LENGTH])
}

pub fn serialize_native() -> Result<Vec<u8>, crate::bytesrepr::Error> {
    let mut fields = BTreeMap::new();
    let tag_field_bytes = NATIVE_TAG.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(tag_field_bytes));

    serialize_fields_map(fields)
}

pub fn serialize_stored(
    id: &TransactionInvocationTarget,
    runtime: &TransactionRuntime,
) -> Result<Vec<u8>, crate::bytesrepr::Error> {
    let mut fields = BTreeMap::new();

    let tag_field_bytes = STORED_TAG.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(tag_field_bytes));

    let id_field_bytes = id.to_bytes()?;
    fields.insert(ID_FIELD_META_INDEX, Bytes::from(id_field_bytes));

    let runtime_field_bytes = runtime.to_bytes()?;
    fields.insert(RUNTIME_FIELD_META_INDEX, Bytes::from(runtime_field_bytes));

    serialize_fields_map(fields)
}

pub fn stored_serialized_length(
    id: &TransactionInvocationTarget,
    runtime: &TransactionRuntime,
) -> usize {
    serialized_length_for_field_sizes(vec![
        U8_SERIALIZED_LENGTH,
        id.serialized_length(),
        runtime.serialized_length(),
    ])
}

pub fn serialize_session(
    module_bytes: &Bytes,
    runtime: &TransactionRuntime,
) -> Result<Vec<u8>, crate::bytesrepr::Error> {
    let mut fields = BTreeMap::new();

    let tag_field_bytes = SESSION_TAG.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(tag_field_bytes));

    let module_bytes_bytes = module_bytes.to_bytes()?;
    fields.insert(
        MODULE_BYTES_FIELD_META_INDEX,
        Bytes::from(module_bytes_bytes),
    );

    let runtime_field_bytes = runtime.to_bytes()?;
    fields.insert(RUNTIME_FIELD_META_INDEX, Bytes::from(runtime_field_bytes));

    serialize_fields_map(fields)
}

pub fn session_serialized_length(module_bytes: &Bytes, runtime: &TransactionRuntime) -> usize {
    serialized_length_for_field_sizes(vec![
        U8_SERIALIZED_LENGTH,
        module_bytes.serialized_length(),
        runtime.serialized_length(),
    ])
}

pub fn deserialize_transaction_target(
    bytes: &[u8],
) -> Result<(TransactionTarget, &[u8]), bytesrepr::Error> {
    let (mut data_map, remainder) = deserialize_fields_map(bytes)?;
    let tag = consume_field::<u8>(&mut data_map, TAG_FIELD_META_INDEX)?;
    match tag {
        NATIVE_TAG => Ok((TransactionTarget::Native, remainder)),
        STORED_TAG => {
            let id =
                consume_field::<TransactionInvocationTarget>(&mut data_map, ID_FIELD_META_INDEX)?;
            let runtime =
                consume_field::<TransactionRuntime>(&mut data_map, RUNTIME_FIELD_META_INDEX)?;
            Ok((TransactionTarget::Stored { id, runtime }, remainder))
        }
        SESSION_TAG => {
            let module_bytes =
                consume_field::<Bytes>(&mut data_map, MODULE_BYTES_FIELD_META_INDEX)?;
            let runtime =
                consume_field::<TransactionRuntime>(&mut data_map, RUNTIME_FIELD_META_INDEX)?;
            Ok((
                TransactionTarget::Session {
                    module_bytes,
                    runtime,
                },
                remainder,
            ))
        }
        _ => Err(bytesrepr::Error::Formatting),
    }
}
