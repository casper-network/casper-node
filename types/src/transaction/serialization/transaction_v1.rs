use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    Approval, Digest, TransactionV1Body, TransactionV1Header,
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};

use super::{
    consume_field, deserialize_fields_map, serialize_fields_map, serialized_length_for_field_sizes,
    TRANSACTION_V1_SERIALIZATION_VERSION,
};

const SERIALIZATION_VERSION_INDEX: u16 = 0;
const HASH_FIELD_META_INDEX: u16 = 1;
const HEADER_FIELD_META_INDEX: u16 = 2;
const BODY_FIELD_META_INDEX: u16 = 3;
const APPROVALS_FIELD_META_INDEX: u16 = 4;

pub struct TransactionV1BinaryToBytes<'a> {
    pub hash: &'a Digest,
    pub header: &'a TransactionV1Header,
    pub body: &'a TransactionV1Body,
    pub approvals: &'a BTreeSet<Approval>,
}

impl<'a> TransactionV1BinaryToBytes<'a> {
    pub fn new(
        hash: &'a Digest,
        header: &'a TransactionV1Header,
        body: &'a TransactionV1Body,
        approvals: &'a BTreeSet<Approval>,
    ) -> Self {
        TransactionV1BinaryToBytes::<'a> {
            hash,
            header,
            body,
            approvals,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TransactionV1BinaryFromBytes {
    pub hash: Digest,
    pub header: TransactionV1Header,
    pub body: TransactionV1Body,
    pub approvals: BTreeSet<Approval>,
}

impl TransactionV1BinaryFromBytes {
    pub fn new(
        hash: Digest,
        header: TransactionV1Header,
        body: TransactionV1Body,
        approvals: BTreeSet<Approval>,
    ) -> Self {
        TransactionV1BinaryFromBytes {
            hash,
            header,
            body,
            approvals,
        }
    }
}

impl<'a> ToBytes for TransactionV1BinaryToBytes<'a> {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let mut fields = BTreeMap::new();
        let version_bytes = TRANSACTION_V1_SERIALIZATION_VERSION.to_bytes()?;
        fields.insert(SERIALIZATION_VERSION_INDEX, Bytes::from(version_bytes));

        let hash_field_bytes = self.hash.to_bytes()?;
        fields.insert(HASH_FIELD_META_INDEX, Bytes::from(hash_field_bytes));

        let header_field_bytes = self.header.to_bytes()?;
        fields.insert(HEADER_FIELD_META_INDEX, Bytes::from(header_field_bytes));

        let body_field_bytes = self.body.to_bytes()?;
        fields.insert(BODY_FIELD_META_INDEX, Bytes::from(body_field_bytes));

        let approvals_bytes = self.approvals.to_bytes()?;
        fields.insert(APPROVALS_FIELD_META_INDEX, Bytes::from(approvals_bytes));

        serialize_fields_map(fields)
    }

    fn serialized_length(&self) -> usize {
        serialized_length_for_field_sizes(vec![
            TRANSACTION_V1_SERIALIZATION_VERSION.serialized_length(),
            self.approvals.serialized_length(),
            self.hash.serialized_length(),
            self.header.serialized_length(),
            self.body.serialized_length(),
        ])
    }
}

impl FromBytes for TransactionV1BinaryFromBytes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), crate::bytesrepr::Error> {
        let (mut data_map, remainder) = deserialize_fields_map(bytes)?;
        let serialization_version =
            consume_field::<u8>(&mut data_map, SERIALIZATION_VERSION_INDEX)?;
        if serialization_version != TRANSACTION_V1_SERIALIZATION_VERSION {
            return Err(bytesrepr::Error::Formatting);
        }
        let hash = consume_field::<Digest>(&mut data_map, HASH_FIELD_META_INDEX)?;
        let header = consume_field::<TransactionV1Header>(&mut data_map, HEADER_FIELD_META_INDEX)?;
        let body = consume_field::<TransactionV1Body>(&mut data_map, BODY_FIELD_META_INDEX)?;
        let approvals =
            consume_field::<BTreeSet<Approval>>(&mut data_map, APPROVALS_FIELD_META_INDEX)?;

        if !data_map.is_empty() {
            return Err(bytesrepr::Error::Formatting);
        }

        let binary = TransactionV1BinaryFromBytes::new(hash, header, body, approvals);

        Ok((binary, remainder))
    }
}
