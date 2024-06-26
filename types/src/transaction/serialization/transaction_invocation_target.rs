use crate::{
    bytesrepr::{self, Bytes, Error, ToBytes, U8_SERIALIZED_LENGTH},
    EntityVersion, HashAddr, PackageAddr, TransactionInvocationTarget,
};
use alloc::{collections::BTreeMap, string::String, vec::Vec};

use super::{
    consume_field, deserialize_fields_map, serialize_fields_map, serialized_length_for_field_sizes,
};

pub const INVOCABLE_ENTITY_TAG: u8 = 0;
pub const INVOCABLE_ENTITY_ALIAS_TAG: u8 = 1;
pub const PACKAGE_TAG: u8 = 2;
pub const PACKAGE_ALIAS_TAG: u8 = 3;

const TAG_FIELD_META_INDEX: u16 = 1;
const INVOCABLE_ENTITY_HASH_META_INDEX: u16 = 2;
const INVOCABLE_ENTITY_ALIAS_META_INDEX: u16 = 3;
const PACKAGE_HASH_ADDR_META_INDEX: u16 = 4;
const PACKAGE_HASH_VERSION_META_INDEX: u16 = 5;
const PACKAGE_NAME_NAME_META_INDEX: u16 = 6;
const PACKAGE_NAME_VERSION_META_INDEX: u16 = 7;

fn tag_only_fields_map(variant_tag: u8) -> Result<BTreeMap<u16, Bytes>, Error> {
    let mut fields = BTreeMap::new();
    let variant_tag_bytes = variant_tag.to_bytes()?;
    fields.insert(TAG_FIELD_META_INDEX, Bytes::from(variant_tag_bytes));
    Ok(fields)
}

pub fn serialize_by_hash(hash_value: &HashAddr) -> Result<Vec<u8>, bytesrepr::Error> {
    let mut fields = tag_only_fields_map(INVOCABLE_ENTITY_TAG)?;
    let hash_value_bytes = hash_value.to_bytes()?;
    fields.insert(
        INVOCABLE_ENTITY_HASH_META_INDEX,
        Bytes::from(hash_value_bytes),
    );
    serialize_fields_map(fields)
}

pub fn serialize_by_name(name: &str) -> Result<Vec<u8>, bytesrepr::Error> {
    let mut fields = tag_only_fields_map(INVOCABLE_ENTITY_ALIAS_TAG)?;
    let name_bytes = name.to_bytes()?;
    fields.insert(INVOCABLE_ENTITY_ALIAS_META_INDEX, Bytes::from(name_bytes));
    serialize_fields_map(fields)
}

pub fn serialize_by_package_hash(
    addr: &PackageAddr,
    version: &Option<EntityVersion>,
) -> Result<Vec<u8>, bytesrepr::Error> {
    let mut fields = tag_only_fields_map(PACKAGE_TAG)?;
    let addr_bytes = addr.to_bytes()?;
    fields.insert(PACKAGE_HASH_ADDR_META_INDEX, Bytes::from(addr_bytes));
    let version_bytes = version.to_bytes()?;
    fields.insert(PACKAGE_HASH_VERSION_META_INDEX, Bytes::from(version_bytes));
    serialize_fields_map(fields)
}

pub fn serialize_by_package_name(
    name: &str,
    version: &Option<EntityVersion>,
) -> Result<Vec<u8>, bytesrepr::Error> {
    let mut fields = tag_only_fields_map(PACKAGE_ALIAS_TAG)?;
    let name_bytes = name.to_bytes()?;
    fields.insert(PACKAGE_NAME_NAME_META_INDEX, Bytes::from(name_bytes));
    let version_bytes = version.to_bytes()?;
    fields.insert(PACKAGE_NAME_VERSION_META_INDEX, Bytes::from(version_bytes));
    serialize_fields_map(fields)
}

pub fn by_hash_serialized_length(addr: &HashAddr) -> usize {
    serialized_length_for_field_sizes(vec![U8_SERIALIZED_LENGTH, addr.serialized_length()])
}
pub fn by_name_serialized_length(name: &str) -> usize {
    serialized_length_for_field_sizes(vec![U8_SERIALIZED_LENGTH, name.serialized_length()])
}
pub fn by_package_hash_serialized_length(
    addr: &PackageAddr,
    version: &Option<EntityVersion>,
) -> usize {
    serialized_length_for_field_sizes(vec![
        U8_SERIALIZED_LENGTH,
        addr.serialized_length(),
        version.serialized_length(),
    ])
}

pub fn by_package_name_serialized_length(name: &str, version: &Option<EntityVersion>) -> usize {
    serialized_length_for_field_sizes(vec![
        U8_SERIALIZED_LENGTH,
        name.serialized_length(),
        version.serialized_length(),
    ])
}

pub fn deserialize_transaction_invocation_target(
    bytes: &[u8],
) -> Result<(TransactionInvocationTarget, &[u8]), Error> {
    let (mut data_map, remainder) = deserialize_fields_map(bytes)?;

    let tag = consume_field::<u8>(&mut data_map, TAG_FIELD_META_INDEX)?;

    let to_ret = match tag {
        INVOCABLE_ENTITY_TAG => {
            let addr = consume_field::<HashAddr>(&mut data_map, INVOCABLE_ENTITY_HASH_META_INDEX)?;
            Ok((TransactionInvocationTarget::ByHash(addr), remainder))
        }
        INVOCABLE_ENTITY_ALIAS_TAG => {
            let alias = consume_field::<String>(&mut data_map, INVOCABLE_ENTITY_ALIAS_META_INDEX)?;
            Ok((TransactionInvocationTarget::ByName(alias), remainder))
        }
        PACKAGE_TAG => {
            let addr = consume_field::<PackageAddr>(&mut data_map, PACKAGE_HASH_ADDR_META_INDEX)?;
            let version =
                consume_field::<Option<u32>>(&mut data_map, PACKAGE_HASH_VERSION_META_INDEX)?;
            Ok((
                TransactionInvocationTarget::ByPackageHash { addr, version },
                remainder,
            ))
        }
        PACKAGE_ALIAS_TAG => {
            let name = consume_field::<String>(&mut data_map, PACKAGE_NAME_NAME_META_INDEX)?;
            let version =
                consume_field::<Option<u32>>(&mut data_map, PACKAGE_NAME_VERSION_META_INDEX)?;
            Ok((
                TransactionInvocationTarget::ByPackageName { name, version },
                remainder,
            ))
        }
        _ => Err(bytesrepr::Error::Formatting),
    };

    if to_ret.is_ok() && !data_map.is_empty() {
        return Err(bytesrepr::Error::Formatting);
    }

    to_ret
}
