use crate::{
    bytesrepr::{self, Bytes, ToBytes},
    RuntimeArgs, TransactionEntryPoint, TransactionScheduling, TransactionTarget,
    TransactionV1Body,
};
use alloc::{collections::BTreeMap, vec::Vec};

use super::{
    consume_field, deserialize_fields_map, serialize_fields_map, serialized_length_for_field_sizes,
};

const ARGS_FIELD_META_INDEX: u16 = 1;
const TARGET_FIELD_META_INDEX: u16 = 2;
const ENTRY_POINT_FIELD_META_INDEX: u16 = 3;
const TRANSACTION_CATEGORY_FIELD_META_INDEX: u16 = 4;
const SCHEDULING_FIELD_META_INDEX: u16 = 5;

pub fn serialize_transaction_body(
    args: &RuntimeArgs,
    target: &TransactionTarget,
    entry_point: &TransactionEntryPoint,
    transaction_category: &u8,
    scheduling: &TransactionScheduling,
) -> Result<Vec<u8>, bytesrepr::Error> {
    let mut fields = BTreeMap::new();

    let args_bytes = args.to_bytes()?;
    fields.insert(ARGS_FIELD_META_INDEX, Bytes::from(args_bytes));

    let target_field_bytes = target.to_bytes()?;
    fields.insert(TARGET_FIELD_META_INDEX, Bytes::from(target_field_bytes));

    let entry_point_bytes = entry_point.to_bytes()?;
    fields.insert(ENTRY_POINT_FIELD_META_INDEX, Bytes::from(entry_point_bytes));

    let transaction_category_bytes = transaction_category.to_bytes()?;
    fields.insert(
        TRANSACTION_CATEGORY_FIELD_META_INDEX,
        Bytes::from(transaction_category_bytes),
    );

    let scheduling_field_bytes = scheduling.to_bytes()?;
    fields.insert(
        SCHEDULING_FIELD_META_INDEX,
        Bytes::from(scheduling_field_bytes),
    );

    serialize_fields_map(fields)
}

pub fn transaction_body_serialized_length(
    args: &RuntimeArgs,
    target: &TransactionTarget,
    entry_point: &TransactionEntryPoint,
    transaction_category: &u8,
    scheduling: &TransactionScheduling,
) -> usize {
    serialized_length_for_field_sizes(vec![
        args.serialized_length(),
        target.serialized_length(),
        entry_point.serialized_length(),
        transaction_category.serialized_length(),
        scheduling.serialized_length(),
    ])
}

pub fn deserialize_transaction_body(
    bytes: &[u8],
) -> Result<(TransactionV1Body, &[u8]), bytesrepr::Error> {
    let (mut data_map, remainder) = deserialize_fields_map(bytes)?;

    let args = consume_field::<RuntimeArgs>(&mut data_map, ARGS_FIELD_META_INDEX)?;

    let target = consume_field::<TransactionTarget>(&mut data_map, TARGET_FIELD_META_INDEX)?;

    let entry_point =
        consume_field::<TransactionEntryPoint>(&mut data_map, ENTRY_POINT_FIELD_META_INDEX)?;

    let transaction_category =
        consume_field::<u8>(&mut data_map, TRANSACTION_CATEGORY_FIELD_META_INDEX)?;

    let scheduling =
        consume_field::<TransactionScheduling>(&mut data_map, SCHEDULING_FIELD_META_INDEX)?;

    if !data_map.is_empty() {
        return Err(bytesrepr::Error::Formatting);
    }

    let body = TransactionV1Body::new(args, target, entry_point, transaction_category, scheduling);

    Ok((body, remainder))
}
