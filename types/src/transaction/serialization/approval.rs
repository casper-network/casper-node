use crate::{
    bytesrepr::{self, Bytes, ToBytes},
    Approval, PublicKey, Signature,
};
use alloc::{collections::BTreeMap, vec::Vec};

use super::{
    consume_field, deserialize_fields_map, serialize_fields_map, serialized_length_for_field_sizes,
};

const SIGNER_META_INDEX: u16 = 1;
const SIGNATURE_META_INDEX: u16 = 2;

pub fn serialize_approval(
    signer: &PublicKey,
    signature: &Signature,
) -> Result<Vec<u8>, bytesrepr::Error> {
    let mut fields = BTreeMap::new();

    let signer_bytes = signer.to_bytes()?;
    fields.insert(SIGNER_META_INDEX, Bytes::from(signer_bytes));

    let signature_bytes = signature.to_bytes()?;
    fields.insert(SIGNATURE_META_INDEX, Bytes::from(signature_bytes));

    serialize_fields_map(fields)
}

pub fn approval_serialized_size(signer: &PublicKey, signature: &Signature) -> usize {
    serialized_length_for_field_sizes(vec![
        signer.serialized_length(),
        signature.serialized_length(),
    ])
}

pub fn deserialize_approval(bytes: &[u8]) -> Result<(Approval, &[u8]), bytesrepr::Error> {
    let (mut data_map, remainder) = deserialize_fields_map(bytes)?;
    let signer = consume_field::<PublicKey>(&mut data_map, SIGNER_META_INDEX)?;
    let signature = consume_field::<Signature>(&mut data_map, SIGNATURE_META_INDEX)?;
    let approval = Approval::new(signer, signature);
    Ok((approval, remainder))
}
