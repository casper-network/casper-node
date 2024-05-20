#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

/// Configuration values associated with V1 Transactions.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct TransactionV1Config {
    /// [0] -> Kind
    /// [1] -> Max serialized length
    /// [2] -> Max args length
    /// [3] -> Max gas limit
    pub lanes: Vec<Vec<u64>>,
}

#[cfg(any(feature = "testing", test))]
impl TransactionV1Config {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let mut lanes = vec![];
        for kind in 0..7 {
            let lane = vec![
                kind as u64,
                rng.gen_range(0..=1_048_576),
                rng.gen_range(0..=1024),
                rng.gen_range(0..=2_500_000_000),
            ];
            lanes.push(lane)
        }

        TransactionV1Config { lanes }
    }
}

#[cfg(any(feature = "std", test))]
impl Default for TransactionV1Config {
    fn default() -> Self {
        let mut lanes = vec![];
        for kind in 0..7 {
            let lane = vec![kind as u64, 1024, 2048, 2_500_000_000];
            lanes.push(lane)
        }

        TransactionV1Config { lanes }
    }
}

impl ToBytes for TransactionV1Config {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.lanes.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.lanes.serialized_length()
    }
}

impl FromBytes for TransactionV1Config {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (lanes, remainder) = FromBytes::from_bytes(bytes)?;
        let config = TransactionV1Config { lanes };
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
