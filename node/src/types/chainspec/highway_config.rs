use datasize::DataSize;
use num::rational::Ratio;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::error;

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    TimeDiff,
};

#[derive(Copy, Clone, DataSize, PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct HighwayConfig {
    #[data_size(skip)]
    pub(crate) finality_threshold_fraction: Ratio<u64>,
    pub(crate) minimum_round_exponent: u8,
    pub(crate) maximum_round_exponent: u8,
    /// The factor by which rewards for a round are multiplied if the greatest summit has ≤50%
    /// quorum, i.e. no finality.
    #[data_size(skip)]
    pub(crate) reduced_reward_multiplier: Ratio<u64>,
}

impl HighwayConfig {
    /// Checks whether the values set in the config make sense and returns `false` if they don't.
    pub(super) fn is_valid(&self) -> bool {
        if self.minimum_round_exponent > self.maximum_round_exponent {
            error!(
                min = %self.minimum_round_exponent,
                max = %self.maximum_round_exponent,
                "minimum round exponent is greater than the maximum round exponent",
            );
            return false;
        }

        if self.finality_threshold_fraction <= Ratio::new(0, 1)
            || self.finality_threshold_fraction >= Ratio::new(1, 1)
        {
            error!(
                ftf = %self.finality_threshold_fraction,
                "finality threshold fraction is not in the range (0, 1)",
            );
            return false;
        }

        if self.reduced_reward_multiplier > Ratio::new(1, 1) {
            error!(
                rrm = %self.reduced_reward_multiplier,
                "reduced reward multiplier is not in the range [0, 1]",
            );
            return false;
        }

        true
    }

    /// Returns the length of the shortest allowed round.
    pub fn min_round_length(&self) -> TimeDiff {
        TimeDiff::from(1 << self.minimum_round_exponent)
    }
}

#[cfg(test)]
impl HighwayConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let finality_threshold_fraction = Ratio::new(rng.gen_range(1..100), 100);
        let minimum_round_exponent = rng.gen_range(0..16);
        let maximum_round_exponent = rng.gen_range(16..22);
        let reduced_reward_multiplier = Ratio::new(rng.gen_range(0..10), 10);

        HighwayConfig {
            finality_threshold_fraction,
            minimum_round_exponent,
            maximum_round_exponent,
            reduced_reward_multiplier,
        }
    }
}

impl ToBytes for HighwayConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.finality_threshold_fraction.to_bytes()?);
        buffer.extend(self.minimum_round_exponent.to_bytes()?);
        buffer.extend(self.maximum_round_exponent.to_bytes()?);
        buffer.extend(self.reduced_reward_multiplier.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.finality_threshold_fraction.serialized_length()
            + self.minimum_round_exponent.serialized_length()
            + self.maximum_round_exponent.serialized_length()
            + self.reduced_reward_multiplier.serialized_length()
    }
}

impl FromBytes for HighwayConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (finality_threshold_fraction, remainder) = Ratio::<u64>::from_bytes(bytes)?;
        let (minimum_round_exponent, remainder) = u8::from_bytes(remainder)?;
        let (maximum_round_exponent, remainder) = u8::from_bytes(remainder)?;
        let (reduced_reward_multiplier, remainder) = Ratio::<u64>::from_bytes(remainder)?;
        let config = HighwayConfig {
            finality_threshold_fraction,
            minimum_round_exponent,
            maximum_round_exponent,
            reduced_reward_multiplier,
        };
        Ok((config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let config = HighwayConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }

    #[test]
    fn toml_roundtrip() {
        let mut rng = crate::new_rng();
        let config = HighwayConfig::random(&mut rng);
        let encoded = toml::to_string_pretty(&config).unwrap();
        let decoded = toml::from_str(&encoded).unwrap();
        assert_eq!(config, decoded);
    }

    #[test]
    fn should_validate_round_exponents() {
        let mut rng = crate::new_rng();
        let mut highway_config = HighwayConfig::random(&mut rng);

        // Should be valid for round exponents where min <= max.
        highway_config.minimum_round_exponent = highway_config.maximum_round_exponent - 1;
        assert!(highway_config.is_valid());
        highway_config.minimum_round_exponent = highway_config.maximum_round_exponent;
        assert!(highway_config.is_valid());

        // Should be invalid for round exponents where min > max.
        highway_config.minimum_round_exponent = highway_config.maximum_round_exponent + 1;
        assert!(!highway_config.is_valid());
    }

    #[test]
    fn should_validate_for_finality_threshold() {
        let mut rng = crate::new_rng();
        let mut highway_config = HighwayConfig::random(&mut rng);

        // Should be valid for FTT > 0 and < 1.
        highway_config.finality_threshold_fraction = Ratio::new(1, u64::MAX);
        assert!(highway_config.is_valid());
        highway_config.finality_threshold_fraction = Ratio::new(u64::MAX - 1, u64::MAX);
        assert!(highway_config.is_valid());

        // Should be invalid for FTT == 0 or >= 1.
        highway_config.finality_threshold_fraction = Ratio::new(0, 1);
        assert!(!highway_config.is_valid());
        highway_config.finality_threshold_fraction = Ratio::new(1, 1);
        assert!(!highway_config.is_valid());
        highway_config.finality_threshold_fraction = Ratio::new(u64::MAX, u64::MAX);
        assert!(!highway_config.is_valid());
        highway_config.finality_threshold_fraction = Ratio::new(u64::MAX, u64::MAX - 1);
        assert!(!highway_config.is_valid());
    }

    #[test]
    fn should_validate_for_reduced_reward_multiplier() {
        let mut rng = crate::new_rng();
        let mut highway_config = HighwayConfig::random(&mut rng);

        // Should be valid for 0 <= RRM <= 1.
        highway_config.reduced_reward_multiplier = Ratio::new(0, 1);
        assert!(highway_config.is_valid());
        highway_config.reduced_reward_multiplier = Ratio::new(1, 1);
        assert!(highway_config.is_valid());
        highway_config.reduced_reward_multiplier = Ratio::new(u64::MAX, u64::MAX);
        assert!(highway_config.is_valid());

        // Should be invalid for RRM > 1.
        highway_config.reduced_reward_multiplier = Ratio::new(u64::MAX, u64::MAX - 1);
        assert!(!highway_config.is_valid());
    }
}
