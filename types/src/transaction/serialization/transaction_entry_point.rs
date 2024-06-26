use super::{
    consume_field, deserialize_fields_map, serialize_fields_map, serialized_length_for_field_sizes,
};
use crate::{
    bytesrepr::{self, Bytes, ToBytes, U8_SERIALIZED_LENGTH},
    TransactionEntryPoint,
};
use alloc::{collections::BTreeMap, string::String, vec::Vec};

pub const CUSTOM_TAG: u8 = 0;
pub const TRANSFER_TAG: u8 = 1;
pub const ADD_BID_TAG: u8 = 2;
pub const WITHDRAW_BID_TAG: u8 = 3;
pub const DELEGATE_TAG: u8 = 4;
pub const UNDELEGATE_TAG: u8 = 5;
pub const REDELEGATE_TAG: u8 = 6;
pub const ACTIVATE_BID_TAG: u8 = 7;
pub const CHANGE_BID_PUBLIC_KEY_TAG: u8 = 8;
pub const CALL_TAG: u8 = 9;

const TAG_FIELD_META_INDEX: u16 = 1;
const ENTRY_POINT_META_INDEX: u16 = 2;

pub fn serialize_tag_only_variant(variant_tag: u8) -> Result<Vec<u8>, bytesrepr::Error> {
    tag_only_fields_map(variant_tag)?.to_bytes()
}

fn tag_only_fields_map(variant_tag: u8) -> Result<BTreeMap<u16, Bytes>, bytesrepr::Error> {
    let mut fields = BTreeMap::new();
    let variant_tag_bytes = variant_tag.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(variant_tag_bytes));
    Ok(fields)
}

pub fn custom_serialized_length(name: &str) -> usize {
    serialized_length_for_field_sizes(vec![U8_SERIALIZED_LENGTH, name.serialized_length()])
}

pub fn serialize_custom_entry_point(
    variant_tag: u8,
    custom_name: &str,
) -> Result<Vec<u8>, bytesrepr::Error> {
    let mut fields = tag_only_fields_map(variant_tag)?;
    let custom_name_bytes = custom_name.to_bytes()?;
    fields.insert(ENTRY_POINT_META_INDEX, Bytes::from(custom_name_bytes));
    serialize_fields_map(fields)
}

pub fn deserialize_transaction_entry_point(
    bytes: &[u8],
) -> Result<(TransactionEntryPoint, &[u8]), bytesrepr::Error> {
    let (mut data_map, remainder) = deserialize_fields_map(bytes)?;
    let tag = consume_field::<u8>(&mut data_map, TAG_FIELD_META_INDEX)?;
    let to_ret = match tag {
        CALL_TAG => Ok((TransactionEntryPoint::Call, remainder)),
        CUSTOM_TAG => {
            let entry_point = consume_field::<String>(&mut data_map, ENTRY_POINT_META_INDEX)?;
            Ok((TransactionEntryPoint::Custom(entry_point), remainder))
        }
        TRANSFER_TAG => Ok((TransactionEntryPoint::Transfer, remainder)),
        ADD_BID_TAG => Ok((TransactionEntryPoint::AddBid, remainder)),
        WITHDRAW_BID_TAG => Ok((TransactionEntryPoint::WithdrawBid, remainder)),
        DELEGATE_TAG => Ok((TransactionEntryPoint::Delegate, remainder)),
        UNDELEGATE_TAG => Ok((TransactionEntryPoint::Undelegate, remainder)),
        REDELEGATE_TAG => Ok((TransactionEntryPoint::Redelegate, remainder)),
        ACTIVATE_BID_TAG => Ok((TransactionEntryPoint::ActivateBid, remainder)),
        CHANGE_BID_PUBLIC_KEY_TAG => Ok((TransactionEntryPoint::ChangeBidPublicKey, remainder)),
        _ => Err(bytesrepr::Error::Formatting),
    };

    if to_ret.is_ok() && !data_map.is_empty() {
        return Err(bytesrepr::Error::Formatting);
    }

    to_ret
}
