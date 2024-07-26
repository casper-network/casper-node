use super::{BinaryPayload, BinaryPayloadBuilder, TRANSACTION_V1_SERIALIZATION_VERSION};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Approval, Digest, TransactionV1Body, TransactionV1Header,
};
use alloc::{collections::BTreeSet, vec::Vec};

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

    fn serialized_sizes(&self) -> Vec<usize> {
        let serialization_version_len = TRANSACTION_V1_SERIALIZATION_VERSION.serialized_length();
        let approvals_len = self.approvals.serialized_length();
        let hash_len = self.hash.serialized_length();
        let header_len = self.header.serialized_length();
        let body_len = self.body.serialized_length();
        vec![
            serialization_version_len,
            hash_len,
            header_len,
            body_len,
            approvals_len,
        ]
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TransactionV1FromBytes {
    pub hash: Digest,
    pub header: TransactionV1Header,
    pub body: TransactionV1Body,
    pub approvals: BTreeSet<Approval>,
}

impl TransactionV1FromBytes {
    pub fn new(
        hash: Digest,
        header: TransactionV1Header,
        body: TransactionV1Body,
        approvals: BTreeSet<Approval>,
    ) -> Self {
        TransactionV1FromBytes {
            hash,
            header,
            body,
            approvals,
        }
    }
}

impl<'a> ToBytes for TransactionV1BinaryToBytes<'a> {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let expected_payload_sizes = self.serialized_sizes();
        BinaryPayloadBuilder::new(expected_payload_sizes)?
            .add_field(
                SERIALIZATION_VERSION_INDEX,
                &TRANSACTION_V1_SERIALIZATION_VERSION,
            )?
            .add_field(HASH_FIELD_META_INDEX, &self.hash)?
            .add_field(HEADER_FIELD_META_INDEX, &self.header)?
            .add_field(BODY_FIELD_META_INDEX, &self.body)?
            .add_field(APPROVALS_FIELD_META_INDEX, &self.approvals)?
            .binary_payload_bytes()
    }

    fn serialized_length(&self) -> usize {
        BinaryPayload::estimate_size(self.serialized_sizes())
    }
}

impl FromBytes for TransactionV1FromBytes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), crate::bytesrepr::Error> {
        let (binary_payload, remainder) = BinaryPayload::from_bytes(5, bytes)?;
        let window = binary_payload
            .start_consuming()?
            .ok_or(bytesrepr::Error::Formatting)?;
        window.verify_index(SERIALIZATION_VERSION_INDEX)?;
        let (serialization_version, window) = window.deserialize_and_next::<u8>()?;
        if serialization_version != TRANSACTION_V1_SERIALIZATION_VERSION {
            return Err(bytesrepr::Error::Formatting);
        }
        window.verify_index(HASH_FIELD_META_INDEX)?;
        let (hash, window) = window.deserialize_and_next::<Digest>()?;

        window.verify_index(HEADER_FIELD_META_INDEX)?;
        let (header, window) = window.deserialize_and_next::<TransactionV1Header>()?;

        window.verify_index(BODY_FIELD_META_INDEX)?;
        let (body, window) = window.deserialize_and_next::<TransactionV1Body>()?;

        window.verify_index(APPROVALS_FIELD_META_INDEX)?;
        let approvals = window.deserialize_and_stop::<BTreeSet<Approval>>()?;

        let from_bytes = TransactionV1FromBytes::new(hash, header, body, approvals);

        Ok((from_bytes, remainder))
    }
}
