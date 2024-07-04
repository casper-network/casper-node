use super::{consume_field, deserialize_fields_map, serialize_fields_map};
use crate::{
    account::AccountHash,
    bytesrepr::{Bytes, Error, ToBytes},
    InitiatorAddr, PublicKey,
};
use alloc::{collections::BTreeMap, vec::Vec};

pub const PUBLIC_KEY_TAG: u8 = 0;
pub const ACCOUNT_HASH_TAG: u8 = 1;

const TAG_FIELD_META_INDEX: u16 = 0;
const PUBLIC_KEY_META_INDEX: u16 = 1;
const ACCOUNT_HASH_META_INDEX: u16 = 2;

pub fn serialize_public_key(public_key: &PublicKey) -> Result<Vec<u8>, Error> {
    let mut fields = BTreeMap::new();
    let tag_field_bytes = PUBLIC_KEY_TAG.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(tag_field_bytes));

    let public_key_bytes = public_key.to_bytes()?;
    fields.insert(PUBLIC_KEY_META_INDEX, Bytes::from(public_key_bytes));
    serialize_fields_map(fields)
}

pub fn serialize_account_hash(account_hash: &AccountHash) -> Result<Vec<u8>, Error> {
    let mut fields = BTreeMap::new();
    let tag_field_bytes = ACCOUNT_HASH_TAG.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(tag_field_bytes));

    let account_hash_bytes = account_hash.to_bytes()?;
    fields.insert(ACCOUNT_HASH_META_INDEX, Bytes::from(account_hash_bytes));
    serialize_fields_map(fields)
}

pub fn deserialize_initiator_addr(bytes: &[u8]) -> Result<(InitiatorAddr, &[u8]), Error> {
    let (mut data_map, remainder) = deserialize_fields_map(bytes)?;
    let tag = consume_field::<u8>(&mut data_map, TAG_FIELD_META_INDEX)?;
    let to_ret = match tag {
        PUBLIC_KEY_TAG => {
            let public_key = consume_field::<PublicKey>(&mut data_map, PUBLIC_KEY_META_INDEX)?;
            Ok((InitiatorAddr::PublicKey(public_key), remainder))
        }
        ACCOUNT_HASH_TAG => {
            let account_hash =
                consume_field::<AccountHash>(&mut data_map, ACCOUNT_HASH_META_INDEX)?;
            Ok((InitiatorAddr::AccountHash(account_hash), remainder))
        }
        _ => Err(Error::Formatting),
    };
    if to_ret.is_ok() && !data_map.is_empty() {
        return Err(Error::Formatting);
    }
    to_ret
}
