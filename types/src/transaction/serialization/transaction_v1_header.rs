use super::{
    consume_field, deserialize_fields_map, serialize_fields_map, serialized_length_for_field_sizes,
};
use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    Digest, InitiatorAddr, PricingMode, TimeDiff, Timestamp,
};
use alloc::{collections::BTreeMap, string::String, vec::Vec};

const CHAIN_NAME_FIELD_META_INDEX: u16 = 0;
const TIMESTAMP_FIELD_META_INDEX: u16 = 1;
const TTL_FIELD_META_INDEX: u16 = 2;
const BODY_HASH_FIELD_META_INDEX: u16 = 3;
const PRICING_MODE_FIELD_META_INDEX: u16 = 4;
const INITIATOR_ADDR_FIELD_META_INDEX: u16 = 5;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TransactionV1HeaderFromBytes {
    pub chain_name: String,
    pub timestamp: Timestamp,
    pub ttl: TimeDiff,
    pub body_hash: Digest,
    pub pricing_mode: PricingMode,
    pub initiator_addr: InitiatorAddr,
}

impl TransactionV1HeaderFromBytes {
    pub fn new(
        chain_name: String,
        timestamp: Timestamp,
        ttl: TimeDiff,
        body_hash: Digest,
        pricing_mode: PricingMode,
        initiator_addr: InitiatorAddr,
    ) -> Self {
        TransactionV1HeaderFromBytes {
            chain_name,
            timestamp,
            ttl,
            body_hash,
            pricing_mode,
            initiator_addr,
        }
    }
}

impl FromBytes for TransactionV1HeaderFromBytes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), crate::bytesrepr::Error> {
        let (mut data_map, remainder) = deserialize_fields_map(bytes)?;

        let chain_name = consume_field::<String>(&mut data_map, CHAIN_NAME_FIELD_META_INDEX)?;
        let timestamp = consume_field::<Timestamp>(&mut data_map, TIMESTAMP_FIELD_META_INDEX)?;
        let ttl = consume_field::<TimeDiff>(&mut data_map, TTL_FIELD_META_INDEX)?;
        let body_hash = consume_field::<Digest>(&mut data_map, BODY_HASH_FIELD_META_INDEX)?;
        let pricing_mode =
            consume_field::<PricingMode>(&mut data_map, PRICING_MODE_FIELD_META_INDEX)?;
        let initiator_addr =
            consume_field::<InitiatorAddr>(&mut data_map, INITIATOR_ADDR_FIELD_META_INDEX)?;
        if !data_map.is_empty() {
            return Err(bytesrepr::Error::Formatting);
        }

        let binary = TransactionV1HeaderFromBytes::new(
            chain_name,
            timestamp,
            ttl,
            body_hash,
            pricing_mode,
            initiator_addr,
        );

        Ok((binary, remainder))
    }
}

pub struct TransactionV1HeaderToBytes<'a> {
    pub chain_name: &'a String,
    pub timestamp: &'a Timestamp,
    pub ttl: &'a TimeDiff,
    pub body_hash: &'a Digest,
    pub pricing_mode: &'a PricingMode,
    pub initiator_addr: &'a InitiatorAddr,
}

impl<'a> TransactionV1HeaderToBytes<'a> {
    pub fn new(
        chain_name: &'a String,
        timestamp: &'a Timestamp,
        ttl: &'a TimeDiff,
        body_hash: &'a Digest,
        pricing_mode: &'a PricingMode,
        initiator_addr: &'a InitiatorAddr,
    ) -> Self {
        TransactionV1HeaderToBytes {
            chain_name,
            timestamp,
            ttl,
            body_hash,
            pricing_mode,
            initiator_addr,
        }
    }
}

impl<'a> ToBytes for TransactionV1HeaderToBytes<'a> {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let mut fields = BTreeMap::new();

        let chain_name_bytes = self.chain_name.to_bytes()?;
        fields.insert(CHAIN_NAME_FIELD_META_INDEX, Bytes::from(chain_name_bytes));

        let timestamp_field_bytes = self.timestamp.to_bytes()?;
        fields.insert(
            TIMESTAMP_FIELD_META_INDEX,
            Bytes::from(timestamp_field_bytes),
        );

        let ttl_field_bytes = self.ttl.to_bytes()?;
        fields.insert(TTL_FIELD_META_INDEX, Bytes::from(ttl_field_bytes));

        let body_hash_bytes = self.body_hash.to_bytes()?;
        fields.insert(BODY_HASH_FIELD_META_INDEX, Bytes::from(body_hash_bytes));

        let pricing_mode_bytes = self.pricing_mode.to_bytes()?;
        fields.insert(
            PRICING_MODE_FIELD_META_INDEX,
            Bytes::from(pricing_mode_bytes),
        );

        let initiator_addr_bytes = self.initiator_addr.to_bytes()?;
        fields.insert(
            INITIATOR_ADDR_FIELD_META_INDEX,
            Bytes::from(initiator_addr_bytes),
        );

        serialize_fields_map(fields)
    }

    fn serialized_length(&self) -> usize {
        serialized_length_for_field_sizes(vec![
            self.chain_name.serialized_length(),
            self.timestamp.serialized_length(),
            self.ttl.serialized_length(),
            self.body_hash.serialized_length(),
            self.pricing_mode.serialized_length(),
            self.initiator_addr.serialized_length(),
        ])
    }
}
