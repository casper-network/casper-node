use super::serialization::{BinaryPayload, CalltableFromBytes, CalltableToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, ToBytes},
    EraId, Timestamp,
};
use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use macros::{CalltableFromBytes, CalltableToBytes};
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
#[derive(CalltableToBytes, CalltableFromBytes)]
pub enum TransactionScheduling {
    /// No special scheduling applied.
    #[calltable(variant_index = 0)]
    Standard,
    /// Execution should be scheduled for the specified era.
    #[calltable(variant_index = 1)]
    FutureEra(#[calltable(field_index = 1)] EraId),
    /// Execution should be scheduled for the specified timestamp or later.
    #[calltable(variant_index = 2)]
    FutureTimestamp(#[calltable(field_index = 1)] Timestamp),
}

impl TransactionScheduling {
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
