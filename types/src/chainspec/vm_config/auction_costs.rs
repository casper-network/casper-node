//! Costs of the auction system contract.
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// Default cost of the `get_era_validators` auction entry point.
pub const DEFAULT_GET_ERA_VALIDATORS_COST: u64 = 2_500_000_000;
/// Default cost of the `read_seigniorage_recipients` auction entry point.
pub const DEFAULT_READ_SEIGNIORAGE_RECIPIENTS_COST: u64 = 5_000_000_000;
/// Default cost of the `add_bid` auction entry point.
pub const DEFAULT_ADD_BID_COST: u64 = 2_500_000_000;
/// Default cost of the `withdraw_bid` auction entry point.
pub const DEFAULT_WITHDRAW_BID_COST: u64 = 2_500_000_000;
/// Default cost of the `delegate` auction entry point.
pub const DEFAULT_DELEGATE_COST: u64 = DEFAULT_WITHDRAW_BID_COST;
/// Default cost of the `redelegate` auction entry point.
pub const DEFAULT_REDELEGATE_COST: u64 = DEFAULT_WITHDRAW_BID_COST;
/// Default cost of the `undelegate` auction entry point.
pub const DEFAULT_UNDELEGATE_COST: u64 = DEFAULT_WITHDRAW_BID_COST;
/// Default cost of the `run_auction` auction entry point.
pub const DEFAULT_RUN_AUCTION_COST: u64 = 2_500_000_000;
/// Default cost of the `slash` auction entry point.
pub const DEFAULT_SLASH_COST: u64 = 2_500_000_000;
/// Default cost of the `distribute` auction entry point.
pub const DEFAULT_DISTRIBUTE_COST: u64 = 2_500_000_000;
/// Default cost of the `withdraw_delegator_reward` auction entry point.
pub const DEFAULT_WITHDRAW_DELEGATOR_REWARD_COST: u64 = 5_000_000_000;
/// Default cost of the `withdraw_validator_reward` auction entry point.
pub const DEFAULT_WITHDRAW_VALIDATOR_REWARD_COST: u64 = 5_000_000_000;
/// Default cost of the `read_era_id` auction entry point.
pub const DEFAULT_READ_ERA_ID_COST: u64 = 10_000;
/// Default cost of the `activate_bid` auction entry point.
pub const DEFAULT_ACTIVATE_BID_COST: u64 = 10_000;
/// Default cost of the `change_bid_public_key` auction entry point.
pub const DEFAULT_CHANGE_BID_PUBLIC_KEY_COST: u64 = 5_000_000_000;
/// Default cost of the `add_reservations` auction entry point.
pub const DEFAULT_ADD_RESERVATIONS_COST: u64 = DEFAULT_WITHDRAW_BID_COST;
/// Default cost of the `cancel_reservations` auction entry point.
pub const DEFAULT_CANCEL_RESERVATIONS_COST: u64 = DEFAULT_WITHDRAW_BID_COST;

/// Description of the costs of calling auction entrypoints.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct AuctionCosts {
    /// Cost of calling the `get_era_validators` entry point.
    pub get_era_validators: u64,
    /// Cost of calling the `read_seigniorage_recipients` entry point.
    pub read_seigniorage_recipients: u64,
    /// Cost of calling the `add_bid` entry point.
    pub add_bid: u64,
    /// Cost of calling the `withdraw_bid` entry point.
    pub withdraw_bid: u64,
    /// Cost of calling the `delegate` entry point.
    pub delegate: u64,
    /// Cost of calling the `undelegate` entry point.
    pub undelegate: u64,
    /// Cost of calling the `run_auction` entry point.
    pub run_auction: u64,
    /// Cost of calling the `slash` entry point.
    pub slash: u64,
    /// Cost of calling the `distribute` entry point.
    pub distribute: u64,
    /// Cost of calling the `withdraw_delegator_reward` entry point.
    pub withdraw_delegator_reward: u64,
    /// Cost of calling the `withdraw_validator_reward` entry point.
    pub withdraw_validator_reward: u64,
    /// Cost of calling the `read_era_id` entry point.
    pub read_era_id: u64,
    /// Cost of calling the `activate_bid` entry point.
    pub activate_bid: u64,
    /// Cost of calling the `redelegate` entry point.
    pub redelegate: u64,
    /// Cost of calling the `change_bid_public_key` entry point.
    pub change_bid_public_key: u64,
    /// Cost of calling the `add_reservations` entry point.
    pub add_reservations: u64,
    /// Cost of calling the `cancel_reservations` entry point.
    pub cancel_reservations: u64,
}

impl Default for AuctionCosts {
    fn default() -> Self {
        Self {
            get_era_validators: DEFAULT_GET_ERA_VALIDATORS_COST,
            read_seigniorage_recipients: DEFAULT_READ_SEIGNIORAGE_RECIPIENTS_COST,
            add_bid: DEFAULT_ADD_BID_COST,
            withdraw_bid: DEFAULT_WITHDRAW_BID_COST,
            delegate: DEFAULT_DELEGATE_COST,
            undelegate: DEFAULT_UNDELEGATE_COST,
            run_auction: DEFAULT_RUN_AUCTION_COST,
            slash: DEFAULT_SLASH_COST,
            distribute: DEFAULT_DISTRIBUTE_COST,
            withdraw_delegator_reward: DEFAULT_WITHDRAW_DELEGATOR_REWARD_COST,
            withdraw_validator_reward: DEFAULT_WITHDRAW_VALIDATOR_REWARD_COST,
            read_era_id: DEFAULT_READ_ERA_ID_COST,
            activate_bid: DEFAULT_ACTIVATE_BID_COST,
            redelegate: DEFAULT_REDELEGATE_COST,
            change_bid_public_key: DEFAULT_CHANGE_BID_PUBLIC_KEY_COST,
            add_reservations: DEFAULT_ADD_RESERVATIONS_COST,
            cancel_reservations: DEFAULT_CANCEL_RESERVATIONS_COST,
        }
    }
}

impl ToBytes for AuctionCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        let Self {
            get_era_validators,
            read_seigniorage_recipients,
            add_bid,
            withdraw_bid,
            delegate,
            undelegate,
            run_auction,
            slash,
            distribute,
            withdraw_delegator_reward,
            withdraw_validator_reward,
            read_era_id,
            activate_bid,
            redelegate,
            change_bid_public_key,
            add_reservations,
            cancel_reservations,
        } = self;

        ret.append(&mut get_era_validators.to_bytes()?);
        ret.append(&mut read_seigniorage_recipients.to_bytes()?);
        ret.append(&mut add_bid.to_bytes()?);
        ret.append(&mut withdraw_bid.to_bytes()?);
        ret.append(&mut delegate.to_bytes()?);
        ret.append(&mut undelegate.to_bytes()?);
        ret.append(&mut run_auction.to_bytes()?);
        ret.append(&mut slash.to_bytes()?);
        ret.append(&mut distribute.to_bytes()?);
        ret.append(&mut withdraw_delegator_reward.to_bytes()?);
        ret.append(&mut withdraw_validator_reward.to_bytes()?);
        ret.append(&mut read_era_id.to_bytes()?);
        ret.append(&mut activate_bid.to_bytes()?);
        ret.append(&mut redelegate.to_bytes()?);
        ret.append(&mut change_bid_public_key.to_bytes()?);
        ret.append(&mut add_reservations.to_bytes()?);
        ret.append(&mut cancel_reservations.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        let Self {
            get_era_validators,
            read_seigniorage_recipients,
            add_bid,
            withdraw_bid,
            delegate,
            undelegate,
            run_auction,
            slash,
            distribute,
            withdraw_delegator_reward,
            withdraw_validator_reward,
            read_era_id,
            activate_bid,
            redelegate,
            change_bid_public_key,
            add_reservations,
            cancel_reservations,
        } = self;

        get_era_validators.serialized_length()
            + read_seigniorage_recipients.serialized_length()
            + add_bid.serialized_length()
            + withdraw_bid.serialized_length()
            + delegate.serialized_length()
            + undelegate.serialized_length()
            + run_auction.serialized_length()
            + slash.serialized_length()
            + distribute.serialized_length()
            + withdraw_delegator_reward.serialized_length()
            + withdraw_validator_reward.serialized_length()
            + read_era_id.serialized_length()
            + activate_bid.serialized_length()
            + redelegate.serialized_length()
            + change_bid_public_key.serialized_length()
            + add_reservations.serialized_length()
            + cancel_reservations.serialized_length()
    }
}

impl FromBytes for AuctionCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (get_era_validators, rem) = FromBytes::from_bytes(bytes)?;
        let (read_seigniorage_recipients, rem) = FromBytes::from_bytes(rem)?;
        let (add_bid, rem) = FromBytes::from_bytes(rem)?;
        let (withdraw_bid, rem) = FromBytes::from_bytes(rem)?;
        let (delegate, rem) = FromBytes::from_bytes(rem)?;
        let (undelegate, rem) = FromBytes::from_bytes(rem)?;
        let (run_auction, rem) = FromBytes::from_bytes(rem)?;
        let (slash, rem) = FromBytes::from_bytes(rem)?;
        let (distribute, rem) = FromBytes::from_bytes(rem)?;
        let (withdraw_delegator_reward, rem) = FromBytes::from_bytes(rem)?;
        let (withdraw_validator_reward, rem) = FromBytes::from_bytes(rem)?;
        let (read_era_id, rem) = FromBytes::from_bytes(rem)?;
        let (activate_bid, rem) = FromBytes::from_bytes(rem)?;
        let (redelegate, rem) = FromBytes::from_bytes(rem)?;
        let (change_bid_public_key, rem) = FromBytes::from_bytes(rem)?;
        let (add_reservations, rem) = FromBytes::from_bytes(rem)?;
        let (cancel_reservations, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            Self {
                get_era_validators,
                read_seigniorage_recipients,
                add_bid,
                withdraw_bid,
                delegate,
                undelegate,
                run_auction,
                slash,
                distribute,
                withdraw_delegator_reward,
                withdraw_validator_reward,
                read_era_id,
                activate_bid,
                redelegate,
                change_bid_public_key,
                add_reservations,
                cancel_reservations,
            },
            rem,
        ))
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<AuctionCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> AuctionCosts {
        AuctionCosts {
            get_era_validators: rng.gen_range(0..i64::MAX) as u64,
            read_seigniorage_recipients: rng.gen_range(0..i64::MAX) as u64,
            add_bid: rng.gen_range(0..i64::MAX) as u64,
            withdraw_bid: rng.gen_range(0..i64::MAX) as u64,
            delegate: rng.gen_range(0..i64::MAX) as u64,
            undelegate: rng.gen_range(0..i64::MAX) as u64,
            run_auction: rng.gen_range(0..i64::MAX) as u64,
            slash: rng.gen_range(0..i64::MAX) as u64,
            distribute: rng.gen_range(0..i64::MAX) as u64,
            withdraw_delegator_reward: rng.gen_range(0..i64::MAX) as u64,
            withdraw_validator_reward: rng.gen_range(0..i64::MAX) as u64,
            read_era_id: rng.gen_range(0..i64::MAX) as u64,
            activate_bid: rng.gen_range(0..i64::MAX) as u64,
            redelegate: rng.gen_range(0..i64::MAX) as u64,
            change_bid_public_key: rng.gen_range(0..i64::MAX) as u64,
            add_reservations: rng.gen_range(0..i64::MAX) as u64,
            cancel_reservations: rng.gen_range(0..i64::MAX) as u64,
        }
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::prelude::*;

    use super::AuctionCosts;

    prop_compose! {
        pub fn auction_costs_arb()(
            get_era_validators in 0..=(i64::MAX as u64),
            read_seigniorage_recipients in 0..=(i64::MAX as u64),
            add_bid in 0..=(i64::MAX as u64),
            withdraw_bid in 0..=(i64::MAX as u64),
            delegate in 0..=(i64::MAX as u64),
            undelegate in 0..=(i64::MAX as u64),
            run_auction in 0..=(i64::MAX as u64),
            slash in 0..=(i64::MAX as u64),
            distribute in 0..=(i64::MAX as u64),
            withdraw_delegator_reward in 0..=(i64::MAX as u64),
            withdraw_validator_reward in 0..=(i64::MAX as u64),
            read_era_id in 0..=(i64::MAX as u64),
            activate_bid in 0..=(i64::MAX as u64),
            redelegate in 0..=(i64::MAX as u64),
            change_bid_public_key in 0..=(i64::MAX as u64),
            add_reservations in 0..=(i64::MAX as u64),
            cancel_reservations in 0..=(i64::MAX as u64),
        ) -> AuctionCosts {
            AuctionCosts {
                get_era_validators,
                read_seigniorage_recipients,
                add_bid,
                withdraw_bid,
                delegate,
                undelegate,
                run_auction,
                slash,
                distribute,
                withdraw_delegator_reward,
                withdraw_validator_reward,
                read_era_id,
                activate_bid,
                redelegate,
                change_bid_public_key,
                add_reservations,
                cancel_reservations,
            }
        }
    }
}
