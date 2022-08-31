use std::collections::BTreeSet;

use casper_execution_engine::core::engine_state::engine_config::{FeeHandling, RefundHandling};
use datasize::DataSize;
use num::rational::Ratio;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey, TimeDiff,
};
use tracing::error;

#[derive(Clone, DataSize, PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct CoreConfig {
    pub(crate) era_duration: TimeDiff,
    pub(crate) minimum_era_height: u64,
    pub(crate) validator_slots: u32,
    /// Number of eras before an auction actually defines the set of validators.
    /// If you bond with a sufficient bid in era N, you will be a validator in era N +
    /// auction_delay + 1
    pub(crate) auction_delay: u64,
    /// The period after genesis during which a genesis validator's bid is locked.
    pub(crate) locked_funds_period: TimeDiff,
    /// The period in which genesis validator's bid is released over time after it's unlocked.
    pub(crate) vesting_schedule_period: TimeDiff,
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
    /// Auction entrypoints such as "add_bid" or "delegate" are disabled if this flag is set to
    /// `false`. Setting up this option makes sense only for private chains where validator set
    /// rotation is unnecessary.
    pub(crate) allow_auction_bids: bool,
    /// Allows unrestricted transfers between users.
    pub(crate) allow_unrestricted_transfers: bool,
    /// If set to false then consensus doesn't compute rewards and always uses 0.
    pub(crate) compute_rewards: bool,
    /// Administrative accounts are valid option for for a private chain only.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) administrators: BTreeSet<PublicKey>,
    /// Refund handling.
    #[data_size(skip)]
    pub(crate) refund_handling: RefundHandling,
    /// Fee handling.
    pub(crate) fee_handling: FeeHandling,
}

impl CoreConfig {
    /// Checks whether the values set in the config make sense and returns `false` if they don't.
    pub(super) fn is_valid(&self) -> bool {
        let refund_ratio = match &self.refund_handling {
            RefundHandling::Refund { refund_ratio } | RefundHandling::Burn { refund_ratio } => {
                refund_ratio
            }
        };
        if *refund_ratio > Ratio::new(1, 1) {
            error!(%refund_ratio, "refund_ratio is not in the range [0, 1]");
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
        let validator_slots = rng.gen();
        let auction_delay = rng.gen::<u32>() as u64;
        let locked_funds_period = TimeDiff::from(rng.gen_range(600_000..604_800_000));
        let vesting_schedule_period = TimeDiff::from(rng.gen_range(600_000..604_800_000));
        let unbonding_delay = rng.gen_range(1..1_000_000_000);
        let round_seigniorage_rate = Ratio::new(
            rng.gen_range(1..1_000_000_000),
            rng.gen_range(1..1_000_000_000),
        );
        let max_associated_keys = rng.gen();
        let max_runtime_call_stack_height = rng.gen();
        let minimum_delegation_amount = rng.gen::<u32>() as u64;
        let strict_argument_checking = rng.gen();
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
            validator_slots,
            auction_delay,
            locked_funds_period,
            vesting_schedule_period,
            unbonding_delay,
            round_seigniorage_rate,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            strict_argument_checking,
            allow_auction_bids,
            allow_unrestricted_transfers,
            compute_rewards,
            administrators,
            refund_handling,
            fee_handling,
        }
    }
}

impl ToBytes for CoreConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        let CoreConfig {
            era_duration,
            minimum_era_height,
            validator_slots,
            auction_delay,
            locked_funds_period,
            vesting_schedule_period,
            unbonding_delay,
            round_seigniorage_rate,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            strict_argument_checking,
            allow_auction_bids,
            allow_unrestricted_transfers,
            compute_rewards,
            administrators,
            refund_handling,
            fee_handling,
        } = self;

        buffer.extend(era_duration.to_bytes()?);
        buffer.extend(minimum_era_height.to_bytes()?);
        buffer.extend(validator_slots.to_bytes()?);
        buffer.extend(auction_delay.to_bytes()?);
        buffer.extend(locked_funds_period.to_bytes()?);
        buffer.extend(vesting_schedule_period.to_bytes()?);
        buffer.extend(unbonding_delay.to_bytes()?);
        buffer.extend(round_seigniorage_rate.to_bytes()?);
        buffer.extend(max_associated_keys.to_bytes()?);
        buffer.extend(max_runtime_call_stack_height.to_bytes()?);
        buffer.extend(minimum_delegation_amount.to_bytes()?);
        buffer.extend(strict_argument_checking.to_bytes()?);
        buffer.extend(allow_auction_bids.to_bytes()?);
        buffer.extend(allow_unrestricted_transfers.to_bytes()?);
        buffer.extend(compute_rewards.to_bytes()?);
        buffer.extend(administrators.to_bytes()?);
        buffer.extend(refund_handling.to_bytes()?);
        buffer.extend(fee_handling.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let CoreConfig {
            era_duration,
            minimum_era_height,
            validator_slots,
            auction_delay,
            locked_funds_period,
            vesting_schedule_period,
            unbonding_delay,
            round_seigniorage_rate,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            strict_argument_checking,
            allow_auction_bids,
            allow_unrestricted_transfers,
            compute_rewards,
            administrators,
            refund_handling,
            fee_handling,
        } = self;

        era_duration.serialized_length()
            + minimum_era_height.serialized_length()
            + validator_slots.serialized_length()
            + auction_delay.serialized_length()
            + locked_funds_period.serialized_length()
            + vesting_schedule_period.serialized_length()
            + unbonding_delay.serialized_length()
            + round_seigniorage_rate.serialized_length()
            + max_associated_keys.serialized_length()
            + max_runtime_call_stack_height.serialized_length()
            + minimum_delegation_amount.serialized_length()
            + strict_argument_checking.serialized_length()
            + allow_auction_bids.serialized_length()
            + allow_unrestricted_transfers.serialized_length()
            + compute_rewards.serialized_length()
            + administrators.serialized_length()
            + refund_handling.serialized_length()
            + fee_handling.serialized_length()
    }
}

impl FromBytes for CoreConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (era_duration, remainder) = TimeDiff::from_bytes(bytes)?;
        let (minimum_era_height, remainder) = u64::from_bytes(remainder)?;
        let (validator_slots, remainder) = u32::from_bytes(remainder)?;
        let (auction_delay, remainder) = u64::from_bytes(remainder)?;
        let (locked_funds_period, remainder) = TimeDiff::from_bytes(remainder)?;
        let (vesting_schedule_period, remainder) = TimeDiff::from_bytes(remainder)?;
        let (unbonding_delay, remainder) = u64::from_bytes(remainder)?;
        let (round_seigniorage_rate, remainder) = Ratio::<u64>::from_bytes(remainder)?;
        let (max_associated_keys, remainder) = u32::from_bytes(remainder)?;
        let (max_runtime_call_stack_height, remainder) = u32::from_bytes(remainder)?;
        let (minimum_delegation_amount, remainder) = u64::from_bytes(remainder)?;
        let (strict_argument_checking, remainder) = FromBytes::from_bytes(remainder)?;
        let (allow_auction_bids, remainder) = FromBytes::from_bytes(remainder)?;
        let (allow_unrestricted_transfers, remainder) = FromBytes::from_bytes(remainder)?;
        let (compute_rewards, remainder) = FromBytes::from_bytes(remainder)?;
        let (administrators, remainder) = FromBytes::from_bytes(remainder)?;
        let (refund_handling, remainder) = FromBytes::from_bytes(remainder)?;
        let (fee_handling, remainder) = FromBytes::from_bytes(remainder)?;
        let config = CoreConfig {
            era_duration,
            minimum_era_height,
            validator_slots,
            auction_delay,
            locked_funds_period,
            vesting_schedule_period,
            unbonding_delay,
            round_seigniorage_rate,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            strict_argument_checking,
            allow_auction_bids,
            allow_unrestricted_transfers,
            compute_rewards,
            administrators,
            refund_handling,
            fee_handling,
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
}
