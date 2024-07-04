use super::{
    consume_field, deserialize_fields_map, serialize_fields_map, serialized_length_for_field_sizes,
    tag_only_fields_map,
};
use crate::{
    bytesrepr::{self, Bytes, ToBytes, U8_SERIALIZED_LENGTH},
    EraId, Timestamp, TransactionScheduling,
};
use alloc::vec::Vec;

pub const STANDARD_TAG: u8 = 0;
pub const FUTURE_ERA_TAG: u8 = 1;
pub const FUTURE_TIMESTAMP_TAG: u8 = 2;

const TAG_FIELD_META_INDEX: u16 = 0;
const ERA_ID_META_INDEX: u16 = 1;
const TIMESTAMP_META_INDEX: u16 = 2;

pub fn serialize_tag_only_variant(variant_tag: u8) -> Result<Vec<u8>, bytesrepr::Error> {
    tag_only_fields_map(TAG_FIELD_META_INDEX, variant_tag)?.to_bytes()
}

pub fn serialize_future_era(era_id: &EraId) -> Result<Vec<u8>, bytesrepr::Error> {
    let mut fields = tag_only_fields_map(TAG_FIELD_META_INDEX, FUTURE_ERA_TAG)?;
    let era_id_bytes = era_id.to_bytes()?;
    fields.insert(ERA_ID_META_INDEX, Bytes::from(era_id_bytes));
    serialize_fields_map(fields)
}

pub fn serialize_future_timestamp(timestamp: &Timestamp) -> Result<Vec<u8>, bytesrepr::Error> {
    let mut fields = tag_only_fields_map(TAG_FIELD_META_INDEX, FUTURE_TIMESTAMP_TAG)?;
    let timestamp_bytes = timestamp.to_bytes()?;
    fields.insert(TIMESTAMP_META_INDEX, Bytes::from(timestamp_bytes));
    serialize_fields_map(fields)
}

pub fn future_era_serialized_length(era_id: &EraId) -> usize {
    serialized_length_for_field_sizes(vec![U8_SERIALIZED_LENGTH, era_id.serialized_length()])
}

pub fn timestamp_serialized_length(timestamp: &Timestamp) -> usize {
    serialized_length_for_field_sizes(vec![U8_SERIALIZED_LENGTH, timestamp.serialized_length()])
}

pub fn deserialize_transaction_scheduling(
    bytes: &[u8],
) -> Result<(TransactionScheduling, &[u8]), bytesrepr::Error> {
    let (mut data_map, remainder) = deserialize_fields_map(bytes)?;
    let tag = consume_field::<u8>(&mut data_map, TAG_FIELD_META_INDEX)?;

    let to_ret = match tag {
        STANDARD_TAG => Ok((TransactionScheduling::Standard, remainder)),
        FUTURE_ERA_TAG => {
            let era_id = consume_field::<EraId>(&mut data_map, ERA_ID_META_INDEX)?;
            Ok((TransactionScheduling::FutureEra(era_id), remainder))
        }
        FUTURE_TIMESTAMP_TAG => {
            let timestamp = consume_field::<Timestamp>(&mut data_map, TIMESTAMP_META_INDEX)?;
            Ok((TransactionScheduling::FutureTimestamp(timestamp), remainder))
        }
        _ => Err(bytesrepr::Error::Formatting),
    };

    if to_ret.is_ok() && !data_map.is_empty() {
        return Err(bytesrepr::Error::Formatting);
    }

    to_ret
}
