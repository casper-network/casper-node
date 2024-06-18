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

/// Configuration values relevant to Highway consensus.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct HighwayConfig {
    /// The upper limit for Highway round lengths.
    pub maximum_round_length: TimeDiff,
}

impl HighwayConfig {
    /// Checks whether the values set in the config make sense and returns `false` if they don't.
    pub fn is_valid(&self) -> Result<(), String> {
        Ok(())
    }

    /// Returns a random `HighwayConfig`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let maximum_round_length = TimeDiff::from_seconds(rng.gen_range(60..600));

        HighwayConfig {
            maximum_round_length,
        }
    }
}

impl ToBytes for HighwayConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.maximum_round_length.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.maximum_round_length.serialized_length()
    }
}

impl FromBytes for HighwayConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (maximum_round_length, remainder) = TimeDiff::from_bytes(bytes)?;
        let config = HighwayConfig {
            maximum_round_length,
        };
        Ok((config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = TestRng::from_entropy();
        let config = HighwayConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }
}
