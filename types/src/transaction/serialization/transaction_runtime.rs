use crate::{
    bytesrepr::{self, Error},
    TransactionRuntime,
};
use alloc::vec::Vec;

use super::{consume_field, deserialize_fields_map, serialize_fields_map, tag_only_fields_map};

pub const V1_TAG: u8 = 0;
pub const V2_TAG: u8 = 1;

const TAG_FIELD_META_INDEX: u16 = 0;

pub fn serialize_transaction_runtime(runtime: &TransactionRuntime) -> Result<Vec<u8>, Error> {
    let tag = match runtime {
        TransactionRuntime::VmCasperV1 => V1_TAG,
        TransactionRuntime::VmCasperV2 => V2_TAG,
    };
    let fields = tag_only_fields_map(TAG_FIELD_META_INDEX, tag)?;
    serialize_fields_map(fields)
}

pub fn deserialize_transaction_runtime(bytes: &[u8]) -> Result<(TransactionRuntime, &[u8]), Error> {
    let (mut data_map, remainder) = deserialize_fields_map(bytes)?;
    let tag = consume_field::<u8>(&mut data_map, TAG_FIELD_META_INDEX)?;
    let to_ret = match tag {
        V1_TAG => Ok((TransactionRuntime::VmCasperV1, remainder)),
        V2_TAG => Ok((TransactionRuntime::VmCasperV2, remainder)),
        _ => Err(bytesrepr::Error::Formatting),
    };
    if to_ret.is_ok() && !data_map.is_empty() {
        return Err(bytesrepr::Error::Formatting);
    }
    to_ret
}
