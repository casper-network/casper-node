use std::collections::BTreeSet;

use tracing::{error, warn};

use casper_execution_engine::core::engine_state::engine_config::{FeeHandling, RefundHandling};
#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey,
};

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

use casper_types::{system::auction::VESTING_SCHEDULE_LENGTH_MILLIS, ProtocolVersion, TimeDiff};

/// Configuration values associated with the core protocol.
#[derive(Clone, DataSize, PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct CoreConfig {
    /// Duration of an era.
    pub era_duration: TimeDiff,

    /// Minimum era height.
    pub minimum_era_height: u64,

    /// Minimum block time.
    pub minimum_block_time: TimeDiff,

    /// Validator slots.
    pub validator_slots: u32,

    /// Finality threshold fraction.
    #[data_size(skip)]
    pub finality_threshold_fraction: Ratio<u64>,

    /// Protocol version from which nodes are required to hold strict finality signatures.
    pub start_protocol_version_with_strict_finality_signatures_required: ProtocolVersion,

    /// Which finality is required for legacy blocks.
    /// Used to determine finality sufficiency for new joiners syncing blocks created
    /// in a protocol version before
    /// `start_protocol_version_with_strict_finality_signatures_required`.
    pub legacy_required_finality: LegacyRequiredFinality,

    /// Number of eras before an auction actually defines the set of validators.
    /// If you bond with a sufficient bid in era N, you will be a validator in era N +
    /// auction_delay + 1
    pub auction_delay: u64,

    /// The period after genesis during which a genesis validator's bid is locked.
    pub locked_funds_period: TimeDiff,

    /// The period in which genesis validator's bid is released over time after it's unlocked.
    pub vesting_schedule_period: TimeDiff,

    /// The delay in number of eras for paying out the unbonding amount.
    pub unbonding_delay: u64,

    /// Round seigniorage rate represented as a fractional number.
    #[data_size(skip)]
    pub round_seigniorage_rate: Ratio<u64>,

    /// Maximum number of associated keys for a single account.
    pub max_associated_keys: u32,

    /// Maximum height of contract runtime call stack.
    pub max_runtime_call_stack_height: u32,

    /// The minimum bound of motes that can be delegated to a validator.
    pub minimum_delegation_amount: u64,

    /// Global state prune batch size (0 means the feature is off in the current protocol version).
    pub prune_batch_size: u64,

    /// Enables strict arguments checking when calling a contract.
    pub strict_argument_checking: bool,

    /// How many peers to simultaneously ask when sync leaping.
    pub simultaneous_peer_requests: u8,

    /// Which consensus protocol to use.
    pub consensus_protocol: ConsensusProtocolName,

    /// The maximum amount of delegators per validator.
    /// if the value is 0, there is no maximum capacity.
    pub max_delegators_per_validator: u32,
    /// Auction entrypoints such as "add_bid" or "delegate" are disabled if this flag is set to
    /// `false`. Setting up this option makes sense only for private chains where validator set
    /// rotation is unnecessary.
    pub(crate) allow_auction_bids: bool,
    /// Allows unrestricted transfers between users.
    pub(crate) allow_unrestricted_transfers: bool,
    /// If set to false then consensus doesn't compute rewards and always uses 0.
    pub(crate) compute_rewards: bool,
    /// Administrative accounts are valid option for a private chain only.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) administrators: BTreeSet<PublicKey>,
    /// Refund handling.
    #[data_size(skip)]
    pub(crate) refund_handling: RefundHandling,
    /// Fee handling.
    pub(crate) fee_handling: FeeHandling,
}

impl CoreConfig {
    /// The number of eras that have already started and whose validators are still bonded.
    pub fn recent_era_count(&self) -> u64 {
        // Safe to use naked `-` operation assuming `CoreConfig::is_valid()` has been checked.
        self.unbonding_delay - self.auction_delay
    }

    /// Returns `false` if unbonding delay is not greater than auction delay to ensure
    /// that `recent_era_count()` yields a value of at least 1.
    pub fn is_valid(&self) -> bool {
        if self.unbonding_delay <= self.auction_delay {
            warn!(
                unbonding_delay = self.unbonding_delay,
                auction_delay = self.auction_delay,
                "unbonding delay should be greater than auction delay",
            );
            return false;
        }

        // If the era duration is set to zero, we will treat it as explicitly stating that eras
        // should be defined by height only.  Warn only.
        if self.era_duration.millis() > 0
            && self.era_duration.millis()
                < self.minimum_era_height * self.minimum_block_time.millis()
        {
            warn!("era duration is less than minimum era height * round length!");
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

        if self.vesting_schedule_period > TimeDiff::from_millis(VESTING_SCHEDULE_LENGTH_MILLIS) {
            error!(
                vesting_schedule_millis = self.vesting_schedule_period.millis(),
                max_millis = VESTING_SCHEDULE_LENGTH_MILLIS,
                "vesting schedule period too long",
            );
            return false;
        }

        true
    }
}

#[cfg(test)]
impl CoreConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let era_duration = TimeDiff::from_seconds(rng.gen_range(600..604_800));
        let minimum_era_height = rng.gen_range(5..100);
        let minimum_block_time = TimeDiff::from_seconds(rng.gen_range(1..60));
        let validator_slots = rng.gen_range(1..10_000);
        let finality_threshold_fraction = Ratio::new(rng.gen_range(1..100), 100);
        let start_protocol_version_with_strict_finality_signatures_required =
            ProtocolVersion::from_parts(1, rng.gen_range(5..10), rng.gen_range(0..100));
        let legacy_required_finality = rng.gen();
        let auction_delay = rng.gen_range(1..5);
        let locked_funds_period = TimeDiff::from_seconds(rng.gen_range(600..604_800));
        let vesting_schedule_period = TimeDiff::from_seconds(rng.gen_range(600..604_800));
        let unbonding_delay = rng.gen_range((auction_delay + 1)..1_000_000_000);
        let round_seigniorage_rate = Ratio::new(
            rng.gen_range(1..1_000_000_000),
            rng.gen_range(1..1_000_000_000),
        );
        let max_associated_keys = rng.gen();
        let max_runtime_call_stack_height = rng.gen();
        let minimum_delegation_amount = rng.gen::<u32>() as u64;
        let prune_batch_size = rng.gen_range(0..100);
        let strict_argument_checking = rng.gen();
        let simultaneous_peer_requests = rng.gen_range(3..100);
        let consensus_protocol = rng.gen();
        let allow_auction_bids = rng.gen();
        let allow_unrestricted_transfers = rng.gen();
        let compute_rewards = rng.gen();
        let administrators = (0..rng.gen_range(0..=10u32))
            .map(|_| PublicKey::random(rng))
            .collect();
        let refund_handling = {
            let numer = rng.gen_range(0..=100);
            let refund_ratio = Ratio::new(numer, 100);
            RefundHandling::Refund { refund_ratio }
        };

        let fee_handling = if rng.gen() {
            FeeHandling::PayToProposer
        } else {
            FeeHandling::Accumulate
        };

        CoreConfig {
            era_duration,
            minimum_era_height,
            minimum_block_time,
            validator_slots,
            finality_threshold_fraction,
            start_protocol_version_with_strict_finality_signatures_required,
            legacy_required_finality,
            auction_delay,
            locked_funds_period,
            vesting_schedule_period,
            unbonding_delay,
            round_seigniorage_rate,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            prune_batch_size,
            strict_argument_checking,
            simultaneous_peer_requests,
            consensus_protocol,
            max_delegators_per_validator: 0,
            allow_auction_bids,
            administrators,
            allow_unrestricted_transfers,
            compute_rewards,
            refund_handling,
            fee_handling,
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
        buffer.extend(
            self.start_protocol_version_with_strict_finality_signatures_required
                .to_bytes()?,
        );
        buffer.extend(self.legacy_required_finality.to_bytes()?);
        buffer.extend(self.auction_delay.to_bytes()?);
        buffer.extend(self.locked_funds_period.to_bytes()?);
        buffer.extend(self.vesting_schedule_period.to_bytes()?);
        buffer.extend(self.unbonding_delay.to_bytes()?);
        buffer.extend(self.round_seigniorage_rate.to_bytes()?);
        buffer.extend(self.max_associated_keys.to_bytes()?);
        buffer.extend(self.max_runtime_call_stack_height.to_bytes()?);
        buffer.extend(self.minimum_delegation_amount.to_bytes()?);
        buffer.extend(self.prune_batch_size.to_bytes()?);
        buffer.extend(self.strict_argument_checking.to_bytes()?);
        buffer.extend(self.simultaneous_peer_requests.to_bytes()?);
        buffer.extend(self.consensus_protocol.to_bytes()?);
        buffer.extend(self.max_delegators_per_validator.to_bytes()?);
        buffer.extend(self.allow_auction_bids.to_bytes()?);
        buffer.extend(self.allow_unrestricted_transfers.to_bytes()?);
        buffer.extend(self.compute_rewards.to_bytes()?);
        buffer.extend(self.administrators.to_bytes()?);
        buffer.extend(self.refund_handling.to_bytes()?);
        buffer.extend(self.fee_handling.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.era_duration.serialized_length()
            + self.minimum_era_height.serialized_length()
            + self.minimum_block_time.serialized_length()
            + self.validator_slots.serialized_length()
            + self.finality_threshold_fraction.serialized_length()
            + self
                .start_protocol_version_with_strict_finality_signatures_required
                .serialized_length()
            + self.legacy_required_finality.serialized_length()
            + self.auction_delay.serialized_length()
            + self.locked_funds_period.serialized_length()
            + self.vesting_schedule_period.serialized_length()
            + self.unbonding_delay.serialized_length()
            + self.round_seigniorage_rate.serialized_length()
            + self.max_associated_keys.serialized_length()
            + self.max_runtime_call_stack_height.serialized_length()
            + self.minimum_delegation_amount.serialized_length()
            + self.prune_batch_size.serialized_length()
            + self.strict_argument_checking.serialized_length()
            + self.simultaneous_peer_requests.serialized_length()
            + self.consensus_protocol.serialized_length()
            + self.max_delegators_per_validator.serialized_length()
            + self.allow_auction_bids.serialized_length()
            + self.allow_unrestricted_transfers.serialized_length()
            + self.compute_rewards.serialized_length()
            + self.administrators.serialized_length()
            + self.refund_handling.serialized_length()
            + self.fee_handling.serialized_length()
    }
}

impl FromBytes for CoreConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (era_duration, remainder) = TimeDiff::from_bytes(bytes)?;
        let (minimum_era_height, remainder) = u64::from_bytes(remainder)?;
        let (minimum_block_time, remainder) = TimeDiff::from_bytes(remainder)?;
        let (validator_slots, remainder) = u32::from_bytes(remainder)?;
        let (finality_threshold_fraction, remainder) = Ratio::<u64>::from_bytes(remainder)?;
        let (start_protocol_version_with_strict_finality_signatures_required, remainder) =
            ProtocolVersion::from_bytes(remainder)?;
        let (legacy_required_finality, remainder) = LegacyRequiredFinality::from_bytes(remainder)?;
        let (auction_delay, remainder) = u64::from_bytes(remainder)?;
        let (locked_funds_period, remainder) = TimeDiff::from_bytes(remainder)?;
        let (vesting_schedule_period, remainder) = TimeDiff::from_bytes(remainder)?;
        let (unbonding_delay, remainder) = u64::from_bytes(remainder)?;
        let (round_seigniorage_rate, remainder) = Ratio::<u64>::from_bytes(remainder)?;
        let (max_associated_keys, remainder) = u32::from_bytes(remainder)?;
        let (max_runtime_call_stack_height, remainder) = u32::from_bytes(remainder)?;
        let (minimum_delegation_amount, remainder) = u64::from_bytes(remainder)?;
        let (prune_batch_size, remainder) = u64::from_bytes(remainder)?;
        let (strict_argument_checking, remainder) = bool::from_bytes(remainder)?;
        let (simultaneous_peer_requests, remainder) = u8::from_bytes(remainder)?;
        let (consensus_protocol, remainder) = ConsensusProtocolName::from_bytes(remainder)?;
        let (max_delegators_per_validator, remainder) = FromBytes::from_bytes(remainder)?;
        let (allow_auction_bids, remainder) = FromBytes::from_bytes(remainder)?;
        let (allow_unrestricted_transfers, remainder) = FromBytes::from_bytes(remainder)?;
        let (compute_rewards, remainder) = bool::from_bytes(remainder)?;
        let (administrative_accounts, remainder) = FromBytes::from_bytes(remainder)?;
        let (refund_handling, remainder) = FromBytes::from_bytes(remainder)?;
        let (fee_handling, remainder) = FromBytes::from_bytes(remainder)?;
        let config = CoreConfig {
            era_duration,
            minimum_era_height,
            minimum_block_time,
            validator_slots,
            finality_threshold_fraction,
            start_protocol_version_with_strict_finality_signatures_required,
            legacy_required_finality,
            auction_delay,
            locked_funds_period,
            vesting_schedule_period,
            unbonding_delay,
            round_seigniorage_rate,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            prune_batch_size,
            strict_argument_checking,
            simultaneous_peer_requests,
            consensus_protocol,
            max_delegators_per_validator,
            allow_auction_bids,
            allow_unrestricted_transfers,
            compute_rewards,
            administrators: administrative_accounts,
            refund_handling,
            fee_handling,
        };
        Ok((config, remainder))
    }
}

/// Consensus protocol name.
#[derive(Copy, Clone, DataSize, PartialEq, Eq, Debug)]
pub enum ConsensusProtocolName {
    /// Highway.
    Highway,
    /// Zug.
    Zug,
}

impl Serialize for ConsensusProtocolName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ConsensusProtocolName::Highway => "Highway",
            ConsensusProtocolName::Zug => "Zug",
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConsensusProtocolName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match String::deserialize(deserializer)?.to_lowercase().as_str() {
            "highway" => Ok(ConsensusProtocolName::Highway),
            "zug" => Ok(ConsensusProtocolName::Zug),
            _ => Err(DeError::custom("unknown consensus protocol name")),
        }
    }
}

const CONSENSUS_HIGHWAY_TAG: u8 = 0;
const CONSENSUS_ZUG_TAG: u8 = 1;

impl ToBytes for ConsensusProtocolName {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let tag = match self {
            ConsensusProtocolName::Highway => CONSENSUS_HIGHWAY_TAG,
            ConsensusProtocolName::Zug => CONSENSUS_ZUG_TAG,
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
            CONSENSUS_ZUG_TAG => ConsensusProtocolName::Zug,
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
            ConsensusProtocolName::Zug
        }
    }
}

/// Which finality a legacy block needs during a fast sync.
#[derive(Copy, Clone, DataSize, PartialEq, Eq, Debug)]
pub enum LegacyRequiredFinality {
    /// Strict finality: more than 2/3rd of validators.
    Strict,
    /// Weak finality: more than 1/3rd of validators.
    Weak,
    /// Finality always valid.
    Any,
}

impl Serialize for LegacyRequiredFinality {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            LegacyRequiredFinality::Strict => "Strict",
            LegacyRequiredFinality::Weak => "Weak",
            LegacyRequiredFinality::Any => "Any",
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LegacyRequiredFinality {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match String::deserialize(deserializer)?.to_lowercase().as_str() {
            "strict" => Ok(LegacyRequiredFinality::Strict),
            "weak" => Ok(LegacyRequiredFinality::Weak),
            "any" => Ok(LegacyRequiredFinality::Any),
            _ => Err(DeError::custom("unknown legacy required finality")),
        }
    }
}

const LEGACY_REQUIRED_FINALITY_STRICT_TAG: u8 = 0;
const LEGACY_REQUIRED_FINALITY_WEAK_TAG: u8 = 1;
const LEGACY_REQUIRED_FINALITY_ANY_TAG: u8 = 2;

impl ToBytes for LegacyRequiredFinality {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let tag = match self {
            LegacyRequiredFinality::Strict => LEGACY_REQUIRED_FINALITY_STRICT_TAG,
            LegacyRequiredFinality::Weak => LEGACY_REQUIRED_FINALITY_WEAK_TAG,
            LegacyRequiredFinality::Any => LEGACY_REQUIRED_FINALITY_ANY_TAG,
        };
        Ok(vec![tag])
    }

    fn serialized_length(&self) -> usize {
        1
    }
}

impl FromBytes for LegacyRequiredFinality {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            LEGACY_REQUIRED_FINALITY_STRICT_TAG => Ok((LegacyRequiredFinality::Strict, remainder)),
            LEGACY_REQUIRED_FINALITY_WEAK_TAG => Ok((LegacyRequiredFinality::Weak, remainder)),
            LEGACY_REQUIRED_FINALITY_ANY_TAG => Ok((LegacyRequiredFinality::Any, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
impl Distribution<LegacyRequiredFinality> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> LegacyRequiredFinality {
        match rng.gen_range(0..3) {
            0 => LegacyRequiredFinality::Strict,
            1 => LegacyRequiredFinality::Weak,
            2 => LegacyRequiredFinality::Any,
            _not_in_range => unreachable!(),
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
