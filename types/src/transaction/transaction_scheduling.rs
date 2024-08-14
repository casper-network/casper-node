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

#[cfg(doc)]
use super::Transaction;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    EraId, Timestamp,
};

const STANDARD_TAG: u8 = 0;
const FUTURE_ERA_TAG: u8 = 1;
const FUTURE_TIMESTAMP_TAG: u8 = 2;

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
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionScheduling::Standard => STANDARD_TAG.write_bytes(writer),
            TransactionScheduling::FutureEra(era_id) => {
                FUTURE_ERA_TAG.write_bytes(writer)?;
                era_id.write_bytes(writer)
            }
            TransactionScheduling::FutureTimestamp(timestamp) => {
                FUTURE_TIMESTAMP_TAG.write_bytes(writer)?;
                timestamp.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TransactionScheduling::Standard => 0,
                TransactionScheduling::FutureEra(era_id) => era_id.serialized_length(),
                TransactionScheduling::FutureTimestamp(timestamp) => timestamp.serialized_length(),
            }
    }
}

impl FromBytes for TransactionScheduling {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            STANDARD_TAG => Ok((TransactionScheduling::Standard, remainder)),
            FUTURE_ERA_TAG => {
                let (era_id, remainder) = EraId::from_bytes(remainder)?;
                Ok((TransactionScheduling::FutureEra(era_id), remainder))
            }
            FUTURE_TIMESTAMP_TAG => {
                let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
                Ok((TransactionScheduling::FutureTimestamp(timestamp), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&TransactionScheduling::random(rng));
        }
    }
}
