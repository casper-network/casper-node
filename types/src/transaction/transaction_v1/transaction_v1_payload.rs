use core::fmt::{self, Debug, Display, Formatter};

use super::{errors_v1::FieldDeserializationError, PricingMode};
use crate::{
    bytesrepr::{
        Bytes,
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    transaction::serialization::{
        CalltableSerializationEnvelope, CalltableSerializationEnvelopeBuilder,
    },
    DisplayIter, InitiatorAddr, TimeDiff, Timestamp,
};
use alloc::{collections::BTreeMap, string::String, vec::Vec};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

const INITIATOR_ADDR_FIELD_INDEX: u16 = 0;
const TIMESTAMP_FIELD_INDEX: u16 = 1;
const TTL_FIELD_INDEX: u16 = 2;
const CHAIN_NAME_FIELD_INDEX: u16 = 3;
const PRICING_MODE_FIELD_INDEX: u16 = 4;
const FIELDS_FIELD_INDEX: u16 = 5;

const ARGS_MAP_KEY: u16 = 0;
const TARGET_MAP_KEY: u16 = 1;
const ENTRY_POINT_MAP_KEY: u16 = 2;
const SCHEDULING_MAP_KEY: u16 = 3;
const EXPECTED_FIELD_KEYS: [u16; 4] = [
    ARGS_MAP_KEY,
    TARGET_MAP_KEY,
    ENTRY_POINT_MAP_KEY,
    SCHEDULING_MAP_KEY,
];

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(
        description = "A unit of work sent by a client to the network, which when executed can \
        cause global state to be altered."
    )
)]
pub struct TransactionV1Payload {
    initiator_addr: InitiatorAddr,
    timestamp: Timestamp,
    ttl: TimeDiff,
    chain_name: String,
    pricing_mode: PricingMode,
    fields: BTreeMap<u16, Bytes>,
}

impl TransactionV1Payload {
    pub fn new(
        chain_name: String,
        timestamp: Timestamp,
        ttl: TimeDiff,
        pricing_mode: PricingMode,
        initiator_addr: InitiatorAddr,
        fields: BTreeMap<u16, Bytes>,
    ) -> TransactionV1Payload {
        TransactionV1Payload {
            chain_name,
            timestamp,
            ttl,
            pricing_mode,
            initiator_addr,
            fields,
        }
    }

    fn serialized_field_lengths(&self) -> Vec<usize> {
        vec![
            self.initiator_addr.serialized_length(),
            self.timestamp.serialized_length(),
            self.ttl.serialized_length(),
            self.chain_name.serialized_length(),
            self.pricing_mode.serialized_length(),
            self.fields.serialized_length(),
        ]
    }

    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }

    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    pub fn ttl(&self) -> TimeDiff {
        self.ttl
    }

    pub fn pricing_mode(&self) -> &PricingMode {
        &self.pricing_mode
    }

    pub fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
    }

    pub fn fields(&self) -> &BTreeMap<u16, Bytes> {
        &self.fields
    }

    /// Returns the timestamp of when the transaction expires, i.e. `self.timestamp + self.ttl`.
    pub fn expires(&self) -> Timestamp {
        self.timestamp.saturating_add(self.ttl)
    }

    /// Returns `true` if the transaction has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.expires() < current_instant
    }

    pub fn deserialize_field<T: FromBytes>(
        &self,
        index: u16,
    ) -> Result<T, FieldDeserializationError> {
        let field = self
            .fields
            .get(&index)
            .ok_or(FieldDeserializationError::IndexNotExists { index })?;
        let (value, remainder) = T::from_bytes(field)
            .map_err(|error| FieldDeserializationError::FromBytesError { index, error })?;
        if !remainder.is_empty() {
            return Err(FieldDeserializationError::LingeringBytesInField { index });
        }
        Ok(value)
    }

    pub fn number_of_fields(&self) -> usize {
        self.fields.len()
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn invalidate(&mut self) {
        self.chain_name.clear();
    }
}

impl ToBytes for TransactionV1Payload {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let expected_payload_sizes = self.serialized_field_lengths();
        CalltableSerializationEnvelopeBuilder::new(expected_payload_sizes)?
            .add_field(INITIATOR_ADDR_FIELD_INDEX, &self.initiator_addr)?
            .add_field(TIMESTAMP_FIELD_INDEX, &self.timestamp)?
            .add_field(TTL_FIELD_INDEX, &self.ttl)?
            .add_field(CHAIN_NAME_FIELD_INDEX, &self.chain_name)?
            .add_field(PRICING_MODE_FIELD_INDEX, &self.pricing_mode)?
            .add_field(FIELDS_FIELD_INDEX, &self.fields)?
            .binary_payload_bytes()
    }

    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for TransactionV1Payload {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(6, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Formatting)?;

        window.verify_index(INITIATOR_ADDR_FIELD_INDEX)?;
        let (initiator_addr, window) = window.deserialize_and_maybe_next::<InitiatorAddr>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(TIMESTAMP_FIELD_INDEX)?;
        let (timestamp, window) = window.deserialize_and_maybe_next::<Timestamp>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(TTL_FIELD_INDEX)?;
        let (ttl, window) = window.deserialize_and_maybe_next::<TimeDiff>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(CHAIN_NAME_FIELD_INDEX)?;
        let (chain_name, window) = window.deserialize_and_maybe_next::<String>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(PRICING_MODE_FIELD_INDEX)?;
        let (pricing_mode, window) = window.deserialize_and_maybe_next::<PricingMode>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(FIELDS_FIELD_INDEX)?;
        let (fields_as_vec, window) = window.deserialize_and_maybe_next::<Vec<(u16, Bytes)>>()?;
        let fields = build_map(fields_as_vec)?;
        if window.is_some() {
            return Err(Formatting);
        }
        if fields.len() != EXPECTED_FIELD_KEYS.len()
            || EXPECTED_FIELD_KEYS
                .iter()
                .any(|expected_key| !fields.contains_key(expected_key))
        {
            return Err(Formatting);
        }
        let from_bytes = TransactionV1Payload {
            chain_name,
            timestamp,
            ttl,
            pricing_mode,
            initiator_addr,
            fields,
        };

        Ok((from_bytes, remainder))
    }
}

// We need to make sure that the bytes of the `fields` field are serialized in the correct order.
// A BTreeMap is serialized the same as Vec<(K, V)> and it actually, on deserialization, doesn't
// check if the keys are in ascending order. We need to make sure that the incoming transaction
// payload is serialized in a strict way, otherwise we would have trouble with verifying the
// signature(s).
fn build_map(fields_as_vec: Vec<(u16, Bytes)>) -> Result<BTreeMap<u16, Bytes>, Error> {
    let mut ret = BTreeMap::new();
    let mut max_idx: i32 = -1;
    for (key, value) in fields_as_vec {
        let key_signed = key as i32;
        if key_signed <= max_idx {
            return Err(Formatting);
        }
        max_idx = key_signed;
        ret.insert(key, value);
    }

    Ok(ret)
}

impl Display for TransactionV1Payload {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-v1-payload[{}, {}, {}, {}, {}, fields: {}]",
            self.chain_name,
            self.timestamp,
            self.ttl,
            self.pricing_mode,
            self.initiator_addr,
            DisplayIter::new(self.fields.keys())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        testing::TestRng, RuntimeArgs, TransactionEntryPoint, TransactionScheduling,
        TransactionTarget,
    };
    use std::collections::BTreeMap;

    #[test]
    fn reserialize_should_work_with_ascending_ids() {
        let input = vec![
            (0, Bytes::from(vec![1])),
            (1, Bytes::from(vec![2])),
            (4, Bytes::from(vec![3])),
        ];
        let map = build_map(input).expect("Should not fail");
        assert_eq!(
            map,
            BTreeMap::from_iter(vec![
                (0, Bytes::from(vec![1])),
                (1, Bytes::from(vec![2])),
                (4, Bytes::from(vec![3]))
            ])
        );
    }

    #[test]
    fn reserialize_should_fail_when_ids_not_unique() {
        let input = vec![
            (0, Bytes::from(vec![1])),
            (0, Bytes::from(vec![2])),
            (4, Bytes::from(vec![3])),
        ];
        let map_ret = build_map(input);
        assert!(map_ret.is_err());
    }

    #[test]
    fn reserialize_should_fail_when_ids_not_ascending() {
        let input = vec![
            (0, Bytes::from(vec![1])),
            (2, Bytes::from(vec![2])),
            (1, Bytes::from(vec![3])),
        ];
        assert!(build_map(input).is_err());
        let input = vec![
            (0, Bytes::from(vec![1])),
            (2, Bytes::from(vec![2])),
            (0, Bytes::from(vec![3])),
        ];
        assert!(build_map(input).is_err());
        let input = vec![
            (0, Bytes::from(vec![1])),
            (1, Bytes::from(vec![2])),
            (2, Bytes::from(vec![3])),
            (3, Bytes::from(vec![4])),
            (2, Bytes::from(vec![5])),
        ];
        assert!(build_map(input).is_err());
    }

    #[test]
    fn should_fail_if_deserialized_payload_has_too_many_fields() {
        let rng = &mut TestRng::new();
        let (
            args,
            target,
            entry_point,
            scheduling,
            initiator_addr,
            timestamp,
            ttl,
            chain_name,
            pricing_mode,
        ) = random_payload_data(rng);
        let mut fields = BTreeMap::new();
        fields.insert(ARGS_MAP_KEY, args.to_bytes().unwrap().into());
        fields.insert(TARGET_MAP_KEY, target.to_bytes().unwrap().into());
        fields.insert(ENTRY_POINT_MAP_KEY, entry_point.to_bytes().unwrap().into());
        fields.insert(SCHEDULING_MAP_KEY, scheduling.to_bytes().unwrap().into());
        fields.insert(4, 111_u64.to_bytes().unwrap().into());

        let bytes = TransactionV1Payload::new(
            chain_name,
            timestamp,
            ttl,
            pricing_mode,
            initiator_addr,
            fields,
        )
        .to_bytes()
        .unwrap();
        let result = TransactionV1Payload::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn should_fail_if_deserialized_payload_has_unrecognized_fields() {
        let rng = &mut TestRng::new();
        let (
            args,
            target,
            entry_point,
            scheduling,
            initiator_addr,
            timestamp,
            ttl,
            chain_name,
            pricing_mode,
        ) = random_payload_data(rng);
        let mut fields = BTreeMap::new();
        fields.insert(ARGS_MAP_KEY, args.to_bytes().unwrap().into());
        fields.insert(TARGET_MAP_KEY, target.to_bytes().unwrap().into());
        fields.insert(100, entry_point.to_bytes().unwrap().into());
        fields.insert(SCHEDULING_MAP_KEY, scheduling.to_bytes().unwrap().into());

        let bytes = TransactionV1Payload::new(
            chain_name,
            timestamp,
            ttl,
            pricing_mode,
            initiator_addr,
            fields,
        )
        .to_bytes()
        .unwrap();
        let result = TransactionV1Payload::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn should_fail_if_serialized_payoad_has_fields_out_of_order() {
        let rng = &mut TestRng::new();
        let (
            args,
            target,
            entry_point,
            scheduling,
            initiator_addr,
            timestamp,
            ttl,
            chain_name,
            pricing_mode,
        ) = random_payload_data(rng);
        let fields: Vec<(u16, Bytes)> = vec![
            (SCHEDULING_MAP_KEY, scheduling.to_bytes().unwrap().into()),
            (TARGET_MAP_KEY, target.to_bytes().unwrap().into()),
            (ENTRY_POINT_MAP_KEY, entry_point.to_bytes().unwrap().into()),
            (ARGS_MAP_KEY, args.to_bytes().unwrap().into()),
        ];

        let expected_payload_sizes = vec![
            initiator_addr.serialized_length(),
            timestamp.serialized_length(),
            ttl.serialized_length(),
            chain_name.serialized_length(),
            pricing_mode.serialized_length(),
            fields.serialized_length(),
        ];

        let bytes = CalltableSerializationEnvelopeBuilder::new(expected_payload_sizes)
            .unwrap()
            .add_field(INITIATOR_ADDR_FIELD_INDEX, &initiator_addr)
            .unwrap()
            .add_field(TIMESTAMP_FIELD_INDEX, &timestamp)
            .unwrap()
            .add_field(TTL_FIELD_INDEX, &ttl)
            .unwrap()
            .add_field(CHAIN_NAME_FIELD_INDEX, &chain_name)
            .unwrap()
            .add_field(PRICING_MODE_FIELD_INDEX, &pricing_mode)
            .unwrap()
            .add_field(FIELDS_FIELD_INDEX, &fields)
            .unwrap()
            .binary_payload_bytes()
            .unwrap();
        let payload_res = TransactionV1Payload::from_bytes(&bytes);
        assert!(payload_res.is_err());
    }

    fn random_payload_data(
        rng: &mut TestRng,
    ) -> (
        RuntimeArgs,
        TransactionTarget,
        TransactionEntryPoint,
        TransactionScheduling,
        InitiatorAddr,
        Timestamp,
        TimeDiff,
        String,
        PricingMode,
    ) {
        let args = RuntimeArgs::random(rng);
        let target = TransactionTarget::random(rng);
        let entry_point = TransactionEntryPoint::random(rng);
        let scheduling = TransactionScheduling::random(rng);
        let initiator_addr = InitiatorAddr::random(rng);
        let timestamp = Timestamp::now();
        let ttl = TimeDiff::from_millis(1000);
        let chain_name = "chain-name".to_string();
        let pricing_mode = PricingMode::random(rng);
        (
            args,
            target,
            entry_point,
            scheduling,
            initiator_addr,
            timestamp,
            ttl,
            chain_name,
            pricing_mode,
        )
    }
}
