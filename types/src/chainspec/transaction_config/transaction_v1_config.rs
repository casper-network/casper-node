#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

/// Configuration values associated with V1 Transactions.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct TransactionV1Config {
    /// Maximum length in bytes of runtime args per Transaction.
    pub max_args_length: u32,
}

#[cfg(any(feature = "testing", test))]
impl TransactionV1Config {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let max_args_length = rng.gen();

        TransactionV1Config { max_args_length }
    }
}

#[cfg(any(feature = "testing", test))]
impl Default for TransactionV1Config {
    fn default() -> Self {
        TransactionV1Config {
            max_args_length: 1024,
        }
    }
}

impl ToBytes for TransactionV1Config {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.max_args_length.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.max_args_length.serialized_length()
    }
}

impl FromBytes for TransactionV1Config {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_args_length, remainder) = u32::from_bytes(bytes)?;
        let config = TransactionV1Config { max_args_length };
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
