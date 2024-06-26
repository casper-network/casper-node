#[cfg(any(feature = "testing", test))]
use super::serialization::transaction_scheduling::{FUTURE_ERA_TAG, FUTURE_TIMESTAMP_TAG};
use super::serialization::{
    tag_only_serialized_length,
    transaction_scheduling::{
        deserialize_transaction_scheduling, future_era_serialized_length, serialize_future_era,
        serialize_future_timestamp, serialize_tag_only_variant, timestamp_serialized_length,
        STANDARD_TAG,
    },
};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
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

/// The scheduling mode of a [`Transaction`].
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
    /// Returns a random `TransactionScheduling`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            STANDARD_TAG => TransactionScheduling::Standard,
            FUTURE_ERA_TAG => TransactionScheduling::FutureEra(EraId::random(rng)),
            FUTURE_TIMESTAMP_TAG => TransactionScheduling::FutureTimestamp(Timestamp::random(rng)),
            _ => unreachable!(),
        }
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

impl ToBytes for TransactionScheduling {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            TransactionScheduling::Standard => serialize_tag_only_variant(STANDARD_TAG),
            TransactionScheduling::FutureEra(era_id) => serialize_future_era(era_id),
            TransactionScheduling::FutureTimestamp(timestamp) => {
                serialize_future_timestamp(timestamp)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        match self {
            TransactionScheduling::Standard => tag_only_serialized_length(),
            TransactionScheduling::FutureEra(era_id) => future_era_serialized_length(era_id),
            TransactionScheduling::FutureTimestamp(timestamp) => {
                timestamp_serialized_length(timestamp)
            }
        }
    }
}

impl FromBytes for TransactionScheduling {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        deserialize_transaction_scheduling(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gens::transaction_scheduling_arb;
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
