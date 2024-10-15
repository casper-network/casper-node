use super::serialization::CalltableSerializationEnvelope;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    transaction::serialization::CalltableSerializationEnvelopeBuilder,
    EraId, Timestamp,
};
use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

/// The scheduling mode of a [`crate::Transaction`].
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Scheduling mode of a Transaction.")
)]
pub enum TransactionScheduling {
    /// No special scheduling applied.
    Standard,
    /// Execution should be scheduled for the specified era.
    FutureEra(EraId),
    /// Execution should be scheduled for the specified timestamp or later.
    FutureTimestamp(Timestamp),
}

impl TransactionScheduling {
    fn serialized_field_lengths(&self) -> Vec<usize> {
        match self {
            TransactionScheduling::Standard => {
                vec![crate::bytesrepr::U8_SERIALIZED_LENGTH]
            }
            TransactionScheduling::FutureEra(era_id) => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    era_id.serialized_length(),
                ]
            }
            TransactionScheduling::FutureTimestamp(timestamp) => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    timestamp.serialized_length(),
                ]
            }
        }
    }

    /// Returns a random `TransactionScheduling`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => TransactionScheduling::Standard,
            1 => TransactionScheduling::FutureEra(EraId::random(rng)),
            2 => TransactionScheduling::FutureTimestamp(Timestamp::random(rng)),
            _ => unreachable!(),
        }
    }
}

const TAG_FIELD_INDEX: u16 = 0;

const STANDARD_VARIANT: u8 = 0;

const FUTURE_ERA_VARIANT: u8 = 1;
const FUTURE_ERA_ERA_ID_INDEX: u16 = 1;

const FUTURE_TIMESTAMP_VARIANT: u8 = 2;
const FUTURE_TIMESTAMP_TIMESTAMP_INDEX: u16 = 1;

impl ToBytes for TransactionScheduling {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            TransactionScheduling::Standard => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &STANDARD_VARIANT)?
                    .binary_payload_bytes()
            }
            TransactionScheduling::FutureEra(era_id) => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &FUTURE_ERA_VARIANT)?
                    .add_field(FUTURE_ERA_ERA_ID_INDEX, &era_id)?
                    .binary_payload_bytes()
            }
            TransactionScheduling::FutureTimestamp(timestamp) => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &FUTURE_TIMESTAMP_VARIANT)?
                    .add_field(FUTURE_TIMESTAMP_TIMESTAMP_INDEX, &timestamp)?
                    .binary_payload_bytes()
            }
        }
    }
    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for TransactionScheduling {
    fn from_bytes(bytes: &[u8]) -> Result<(TransactionScheduling, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(2, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Formatting)?;
        window.verify_index(0)?;
        let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
        let to_ret = match tag {
            STANDARD_VARIANT => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionScheduling::Standard)
            }
            FUTURE_ERA_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(FUTURE_ERA_ERA_ID_INDEX)?;
                let (era_id, window) = window.deserialize_and_maybe_next::<EraId>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionScheduling::FutureEra(era_id))
            }
            FUTURE_TIMESTAMP_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(FUTURE_TIMESTAMP_TIMESTAMP_INDEX)?;
                let (timestamp, window) = window.deserialize_and_maybe_next::<Timestamp>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionScheduling::FutureTimestamp(timestamp))
            }
            _ => Err(Formatting),
        };
        to_ret.map(|endpoint| (endpoint, remainder))
    }
}

impl Display for TransactionScheduling {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionScheduling::Standard => write!(formatter, "schedule(standard)"),
            TransactionScheduling::FutureEra(era_id) => write!(formatter, "schedule({})", era_id),
            TransactionScheduling::FutureTimestamp(timestamp) => {
                write!(formatter, "schedule({})", timestamp)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr, gens::transaction_scheduling_arb};
    use proptest::prelude::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&TransactionScheduling::random(rng));
        }
    }

    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in transaction_scheduling_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
