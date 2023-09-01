#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    TimeDiff,
};

/// Configuration values associated with Transactions.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct TransactionV1Config {
    /// Maximum time to live any deploy can specify.
    pub max_ttl: TimeDiff,
    /// Maximum possible size in bytes of a single Transaction.
    pub max_transaction_size: u32,
    /// Maximum number of approvals (signatures) allowed in a block across all Transactions (one of
    /// several block limits).
    pub block_max_approval_count: u32,
    /// Maximum length in bytes of runtime args per Transaction.
    pub max_args_length: u32,
}

#[cfg(any(feature = "testing", test))]
impl TransactionV1Config {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let max_ttl = TimeDiff::from_seconds(rng.gen_range(60..3_600));
        let max_transaction_size = rng.gen_range(100_000..1_000_000);
        let block_max_approval_count = rng.gen();
        let max_args_length = rng.gen();

        TransactionV1Config {
            max_ttl,
            max_transaction_size,
            block_max_approval_count,
            max_args_length,
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Default for TransactionV1Config {
    fn default() -> Self {
        let one_day = TimeDiff::from_seconds(86400);
        TransactionV1Config {
            max_ttl: one_day,
            max_transaction_size: 1_048_576,
            block_max_approval_count: 2600,
            max_args_length: 1024,
        }
    }
}

impl ToBytes for TransactionV1Config {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.max_ttl.write_bytes(writer)?;
        self.max_transaction_size.write_bytes(writer)?;
        self.block_max_approval_count.write_bytes(writer)?;
        self.max_args_length.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.max_ttl.serialized_length()
            + self.max_transaction_size.serialized_length()
            + self.block_max_approval_count.serialized_length()
            + self.max_args_length.serialized_length()
    }
}

impl FromBytes for TransactionV1Config {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_ttl, remainder) = TimeDiff::from_bytes(bytes)?;
        let (max_transaction_size, remainder) = u32::from_bytes(remainder)?;
        let (block_max_approval_count, remainder) = u32::from_bytes(remainder)?;
        let (max_args_length, remainder) = u32::from_bytes(remainder)?;
        let config = TransactionV1Config {
            max_ttl,
            max_transaction_size,
            block_max_approval_count,
            max_args_length,
        };
        Ok((config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = TestRng::new();
        let config = TransactionV1Config::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }
}
