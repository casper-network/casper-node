use datasize::DataSize;
use num::rational::Ratio;
#[cfg(test)]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{
    de::{Deserializer, Error as DeError},
    Deserialize, Serialize, Serializer,
};
use tracing::error;

use casper_types::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(test)]
use casper_types::testing::TestRng;

use crate::types::TimeDiff;

#[derive(Copy, Clone, DataSize, PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct CoreConfig {
    pub(crate) era_duration: TimeDiff,
    pub(crate) minimum_era_height: u64,
    pub(crate) minimum_block_time: TimeDiff,
    pub(crate) validator_slots: u32,
    #[data_size(skip)]
    pub(crate) finality_threshold_fraction: Ratio<u64>,
    /// Number of eras before an auction actually defines the set of validators.
    /// If you bond with a sufficient bid in era N, you will be a validator in era N +
    /// auction_delay + 1
    pub(crate) auction_delay: u64,
    /// The period after genesis during which a genesis validator's bid is locked.
    pub(crate) locked_funds_period: TimeDiff,
    /// The delay in number of eras for paying out the the unbonding amount.
    pub(crate) unbonding_delay: u64,
    /// Round seigniorage rate represented as a fractional number.
    #[data_size(skip)]
    pub(crate) round_seigniorage_rate: Ratio<u64>,
    /// Maximum number of associated keys for a single account.
    pub(crate) max_associated_keys: u32,
    /// Maximum height of contract runtime call stack.
    pub(crate) max_runtime_call_stack_height: u32,
    /// The minimum bound of motes that can be delegated to a validator.
    pub(crate) minimum_delegation_amount: u64,
    /// Enables strict arguments checking when calling a contract.
    pub(crate) strict_argument_checking: bool,
    /// Which consensus protocol to use.
    pub(crate) consensus_protocol: ConsensusProtocolName,
}

impl CoreConfig {
    /// Checks whether the values set in the config make sense and returns `false` if they don't.
    pub(super) fn is_valid(&self) -> bool {
        if self.finality_threshold_fraction <= Ratio::new(0, 1)
            || self.finality_threshold_fraction >= Ratio::new(1, 1)
        {
            error!(
                ftf = %self.finality_threshold_fraction,
                "finality threshold fraction is not in the range (0, 1)",
            );
            return false;
        }

        if self.validator_slots == 0 {
            error!("more than 0 validator slots required");
            return false;
        }
        true
    }
}

#[cfg(test)]
impl CoreConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let era_duration = TimeDiff::from(rng.gen_range(600_000..604_800_000));
        let minimum_era_height = rng.gen_range(5..100);
        let minimum_block_time = TimeDiff::from(rng.gen_range(1_000..60_000));
        let validator_slots = rng.gen_range(1..10_000);
        let finality_threshold_fraction = Ratio::new(rng.gen_range(1..100), 100);
        let auction_delay = rng.gen::<u32>() as u64;
        let locked_funds_period = TimeDiff::from(rng.gen_range(600_000..604_800_000));
        let unbonding_delay = rng.gen_range(1..1_000_000_000);
        let round_seigniorage_rate = Ratio::new(
            rng.gen_range(1..1_000_000_000),
            rng.gen_range(1..1_000_000_000),
        );
        let max_associated_keys = rng.gen();
        let max_runtime_call_stack_height = rng.gen();
        let minimum_delegation_amount = rng.gen::<u32>() as u64;
        let strict_argument_checking = rng.gen();
        let consensus_protocol = rng.gen();

        CoreConfig {
            era_duration,
            minimum_era_height,
            minimum_block_time,
            validator_slots,
            finality_threshold_fraction,
            auction_delay,
            locked_funds_period,
            unbonding_delay,
            round_seigniorage_rate,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            strict_argument_checking,
            consensus_protocol,
        }
    }
}

impl ToBytes for CoreConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.era_duration.to_bytes()?);
        buffer.extend(self.minimum_era_height.to_bytes()?);
        buffer.extend(self.minimum_block_time.to_bytes()?);
        buffer.extend(self.validator_slots.to_bytes()?);
        buffer.extend(self.finality_threshold_fraction.to_bytes()?);
        buffer.extend(self.auction_delay.to_bytes()?);
        buffer.extend(self.locked_funds_period.to_bytes()?);
        buffer.extend(self.unbonding_delay.to_bytes()?);
        buffer.extend(self.round_seigniorage_rate.to_bytes()?);
        buffer.extend(self.max_associated_keys.to_bytes()?);
        buffer.extend(self.max_runtime_call_stack_height.to_bytes()?);
        buffer.extend(self.minimum_delegation_amount.to_bytes()?);
        buffer.extend(self.strict_argument_checking.to_bytes()?);
        buffer.extend(self.consensus_protocol.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.era_duration.serialized_length()
            + self.minimum_era_height.serialized_length()
            + self.minimum_block_time.serialized_length()
            + self.validator_slots.serialized_length()
            + self.finality_threshold_fraction.serialized_length()
            + self.auction_delay.serialized_length()
            + self.locked_funds_period.serialized_length()
            + self.unbonding_delay.serialized_length()
            + self.round_seigniorage_rate.serialized_length()
            + self.max_associated_keys.serialized_length()
            + self.max_runtime_call_stack_height.serialized_length()
            + self.minimum_delegation_amount.serialized_length()
            + self.strict_argument_checking.serialized_length()
            + self.consensus_protocol.serialized_length()
    }
}

impl FromBytes for CoreConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (era_duration, remainder) = TimeDiff::from_bytes(bytes)?;
        let (minimum_era_height, remainder) = u64::from_bytes(remainder)?;
        let (minimum_block_time, remainder) = TimeDiff::from_bytes(remainder)?;
        let (validator_slots, remainder) = u32::from_bytes(remainder)?;
        let (finality_threshold_fraction, remainder) = Ratio::<u64>::from_bytes(remainder)?;
        let (auction_delay, remainder) = u64::from_bytes(remainder)?;
        let (locked_funds_period, remainder) = TimeDiff::from_bytes(remainder)?;
        let (unbonding_delay, remainder) = u64::from_bytes(remainder)?;
        let (round_seigniorage_rate, remainder) = Ratio::<u64>::from_bytes(remainder)?;
        let (max_associated_keys, remainder) = u32::from_bytes(remainder)?;
        let (max_runtime_call_stack_height, remainder) = u32::from_bytes(remainder)?;
        let (minimum_delegation_amount, remainder) = u64::from_bytes(remainder)?;
        let (strict_argument_checking, remainder) = bool::from_bytes(remainder)?;
        let (consensus_protocol, remainder) = ConsensusProtocolName::from_bytes(remainder)?;
        let config = CoreConfig {
            era_duration,
            minimum_era_height,
            minimum_block_time,
            validator_slots,
            finality_threshold_fraction,
            auction_delay,
            locked_funds_period,
            unbonding_delay,
            round_seigniorage_rate,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            strict_argument_checking,
            consensus_protocol,
        };
        Ok((config, remainder))
    }
}

#[derive(Copy, Clone, DataSize, PartialEq, Eq, Debug)]
pub(crate) enum ConsensusProtocolName {
    Highway,
    Simple,
}

impl Serialize for ConsensusProtocolName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ConsensusProtocolName::Highway => "Highway",
            ConsensusProtocolName::Simple => "Simple",
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConsensusProtocolName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match String::deserialize(deserializer)?.to_lowercase().as_str() {
            "highway" => Ok(ConsensusProtocolName::Highway),
            "simple" => Ok(ConsensusProtocolName::Simple),
            _ => Err(DeError::custom("unknown consensus protocol name")),
        }
    }
}

const CONSENSUS_HIGHWAY_TAG: u8 = 0;
const CONSENSUS_SIMPLE_TAG: u8 = 1;

impl ToBytes for ConsensusProtocolName {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let tag = match self {
            ConsensusProtocolName::Highway => CONSENSUS_HIGHWAY_TAG,
            ConsensusProtocolName::Simple => CONSENSUS_SIMPLE_TAG,
        };
        Ok(vec![tag])
    }

    fn serialized_length(&self) -> usize {
        1
    }
}

impl FromBytes for ConsensusProtocolName {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let name = match tag {
            CONSENSUS_HIGHWAY_TAG => ConsensusProtocolName::Highway,
            CONSENSUS_SIMPLE_TAG => ConsensusProtocolName::Simple,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((name, remainder))
    }
}

#[cfg(test)]
impl Distribution<ConsensusProtocolName> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ConsensusProtocolName {
        if rng.gen() {
            ConsensusProtocolName::Highway
        } else {
            ConsensusProtocolName::Simple
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let config = CoreConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }

    #[test]
    fn toml_roundtrip() {
        let mut rng = crate::new_rng();
        let config = CoreConfig::random(&mut rng);
        let encoded = toml::to_string_pretty(&config).unwrap();
        let decoded = toml::from_str(&encoded).unwrap();
        assert_eq!(config, decoded);
    }

    #[test]
    fn should_validate_for_finality_threshold() {
        let mut rng = crate::new_rng();
        let mut config = CoreConfig::random(&mut rng);
        // Should be valid for FTT > 0 and < 1.
        config.finality_threshold_fraction = Ratio::new(1, u64::MAX);
        assert!(config.is_valid());
        config.finality_threshold_fraction = Ratio::new(u64::MAX - 1, u64::MAX);
        assert!(config.is_valid());
        // Should be invalid for FTT == 0 or >= 1.
        config.finality_threshold_fraction = Ratio::new(0, 1);
        assert!(!config.is_valid());
        config.finality_threshold_fraction = Ratio::new(1, 1);
        assert!(!config.is_valid());
        config.finality_threshold_fraction = Ratio::new(u64::MAX, u64::MAX);
        assert!(!config.is_valid());
        config.finality_threshold_fraction = Ratio::new(u64::MAX, u64::MAX - 1);
        assert!(!config.is_valid());
    }
}
