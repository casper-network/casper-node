//! Contains implementation of a Auction contract functionality.
mod bid;
mod constants;
mod delegator;
mod detail;
mod era_info;
mod providers;
mod seigniorage_recipient;
mod types;
mod unbonding_purse;

use alloc::{collections::BTreeMap, vec::Vec};

use num_rational::Ratio;

use crate::{
    account::AccountHash,
    system_contract_errors::auction::{Error, Result},
    PublicKey, U512,
};

pub use bid::Bid;
pub use constants::*;
pub use delegator::Delegator;
pub use era_info::*;
pub use providers::{
    AccountProvider, MintProvider, RuntimeProvider, StorageProvider, SystemProvider,
};
pub use seigniorage_recipient::SeigniorageRecipient;
pub use types::*;
pub use unbonding_purse::UnbondingPurse;

/// Bonding auction contract interface
pub trait Auction:
    StorageProvider + SystemProvider + RuntimeProvider + MintProvider + AccountProvider + Sized
{
    /// Returns era_validators.
    ///
    /// Publicly accessible, but intended for periodic use by the PoS contract to update its own
    /// internal data structures recording current and past winners.
    fn get_era_validators(&mut self) -> Result<EraValidators> {
        let snapshot = detail::get_seigniorage_recipients_snapshot(self)?;
        let era_validators = snapshot
            .into_iter()
            .map(|(era_id, recipients)| {
                let validator_weights = recipients
                    .into_iter()
                    .map(|(public_key, bid)| (public_key, bid.total_stake()))
                    .collect::<ValidatorWeights>();
                (era_id, validator_weights)
            })
            .collect::<BTreeMap<EraId, ValidatorWeights>>();
        Ok(era_validators)
    }

    /// Returns validators in era_validators, mapped to their bids or founding stakes, delegation
    /// rates and lists of delegators together with their delegated quantities from delegators.
    /// This function is publicly accessible, but intended for system use by the PoS contract,
    /// because this data is necessary for distributing seigniorage.
    fn read_seigniorage_recipients(&mut self) -> Result<SeigniorageRecipients> {
        // `era_validators` are assumed to be computed already by calling "run_auction" entrypoint.
        let era_index = detail::get_era_id(self)?;
        let mut seigniorage_recipients_snapshot =
            detail::get_seigniorage_recipients_snapshot(self)?;
        let seigniorage_recipients = seigniorage_recipients_snapshot
            .remove(&era_index)
            .ok_or(Error::MissingSeigniorageRecipients)?;
        Ok(seigniorage_recipients)
    }

    /// For a non-founder validator, this adds, or modifies, an entry in the `bids` collection and
    /// calls `bond` in the Mint contract to create (or top off) a bid purse. It also adjusts the
    /// delegation rate.
    fn add_bid(
        &mut self,
        public_key: PublicKey,
        delegation_rate: DelegationRate,
        amount: U512,
    ) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(&public_key, |x| self.blake2b(x));
        if self.get_caller() != account_hash {
            return Err(Error::InvalidPublicKey);
        }

        if amount.is_zero() {
            return Err(Error::BondTooSmall);
        }

        let source = self.get_main_purse()?;

        // Update bids or stakes
        let mut validators = detail::get_bids(self)?;
        let new_amount = match validators.get_mut(&public_key) {
            Some(bid) => {
                self.transfer_purse_to_purse(source, *bid.bonding_purse(), amount)
                    .map_err(|_| Error::TransferToBidPurse)?;
                bid.with_delegation_rate(delegation_rate)
                    .increase_stake(amount)?
            }
            None => {
                let bonding_purse = self.create_purse()?;
                self.transfer_purse_to_purse(source, bonding_purse, amount)
                    .map_err(|_| Error::TransferToBidPurse)?;
                let bid = Bid::unlocked(bonding_purse, amount, delegation_rate);
                validators.insert(public_key, bid);
                amount
            }
        };
        detail::set_bids(self, validators)?;

        Ok(new_amount)
    }

    /// For a non-founder validator, implements essentially the same logic as add_bid, but reducing
    /// the number of tokens and calling unbond in lieu of bond.
    ///
    /// For a founding validator, this function first checks whether they are released, and fails
    /// if they are not.
    ///
    /// The function returns a the new amount of motes remaining in the bid. If the target bid
    /// does not exist, the function call returns an error.
    fn withdraw_bid(&mut self, public_key: PublicKey, amount: U512) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(&public_key, |x| self.blake2b(x));
        if self.get_caller() != account_hash {
            return Err(Error::InvalidPublicKey);
        }

        // Update bids or stakes
        let mut bids = detail::get_bids(self)?;

        let bid = bids.get_mut(&public_key).ok_or(Error::ValidatorNotFound)?;

        let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;

        // Fails if requested amount is greater than either the total stake or the amount of vested
        // stake.
        let new_amount = bid.decrease_stake(amount, era_end_timestamp_millis)?;

        detail::create_unbonding_purse(
            self,
            public_key,
            public_key, // validator is the unbonder
            *bid.bonding_purse(),
            amount,
        )?;

        if new_amount.is_zero() {
            // NOTE: Assumed safe as we're checking for existence above
            bids.remove(&public_key).unwrap();
        }

        detail::set_bids(self, bids)?;

        Ok(new_amount)
    }

    /// Adds a new delegator to delegators, or tops off a current one. If the target validator is
    /// not in founders, the function call returns an error and does nothing.
    ///
    /// The function calls bond in the Mint contract to transfer motes to the validator's purse and
    /// returns a tuple of that purse and the amount of motes contained in it after the transfer.
    fn delegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(&delegator_public_key, |x| self.blake2b(x));
        if self.get_caller() != account_hash {
            return Err(Error::InvalidPublicKey);
        }

        if amount.is_zero() {
            return Err(Error::BondTooSmall);
        }

        let source = self.get_main_purse()?;

        let mut bids = detail::get_bids(self)?;

        let delegators = match bids.get_mut(&validator_public_key) {
            Some(bid) => bid.delegators_mut(),
            None => {
                // Return early if target validator is not in `bids`
                return Err(Error::ValidatorNotFound);
            }
        };

        let new_delegation_amount = match delegators.get_mut(&delegator_public_key) {
            Some(delegator) => {
                self.transfer_purse_to_purse(source, *delegator.bonding_purse(), amount)
                    .map_err(|_| Error::TransferToDelegatorPurse)?;
                delegator.increase_stake(amount)?;
                *delegator.staked_amount()
            }
            None => {
                let bonding_purse = self.create_purse()?;
                self.transfer_purse_to_purse(source, bonding_purse, amount)
                    .map_err(|_| Error::TransferToDelegatorPurse)?;
                let delegator = Delegator::new(amount, bonding_purse, validator_public_key);
                delegators.insert(delegator_public_key, delegator);
                amount
            }
        };

        detail::set_bids(self, bids)?;

        Ok(new_delegation_amount)
    }

    /// Removes an amount of motes (or the entry altogether, if the remaining amount is 0) from
    /// the entry in delegators and calls unbond in the Mint contract to create a new unbonding
    /// purse.
    ///
    /// The arguments are the delegator’s key, the validator key and quantity of motes and
    /// returns a tuple of the unbonding purse along with the remaining bid amount.
    fn undelegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(&delegator_public_key, |x| self.blake2b(x));
        if self.get_caller() != account_hash {
            return Err(Error::InvalidPublicKey);
        }

        let mut bids = detail::get_bids(self)?;

        let delegators = match bids.get_mut(&validator_public_key) {
            Some(bid) => bid.delegators_mut(),
            None => {
                // Return early if target validator is not in `bids`
                return Err(Error::ValidatorNotFound);
            }
        };

        let new_amount = match delegators.get_mut(&delegator_public_key) {
            Some(delegator) => {
                detail::create_unbonding_purse(
                    self,
                    validator_public_key,
                    delegator_public_key,
                    *delegator.bonding_purse(),
                    amount,
                )?;
                let updated_stake = delegator.decrease_stake(amount)?;
                if updated_stake == U512::zero() {
                    delegators.remove(&delegator_public_key);
                };
                updated_stake
            }
            None => return Err(Error::DelegatorNotFound),
        };

        detail::set_bids(self, bids)?;

        Ok(new_amount)
    }

    /// Slashes each validator.
    ///
    /// This can be only invoked through a system call.
    fn slash(&mut self, validator_public_keys: Vec<PublicKey>) -> Result<()> {
        if self.get_caller() != SYSTEM_ACCOUNT {
            return Err(Error::InvalidCaller);
        }

        let mut burned_amount: U512 = U512::zero();

        let mut bids = detail::get_bids(self)?;
        let mut bids_modified = false;

        let mut unbonding_purses: UnbondingPurses = detail::get_unbonding_purses(self)?;
        let mut unbonding_purses_modified = false;
        for validator_public_key in validator_public_keys {
            // Remove bid for given validator, saving its delegators
            if let Some(bid) = bids.remove(&validator_public_key) {
                burned_amount += *bid.staked_amount();
                bids_modified = true;
            };

            // Update unbonding entries for given validator
            if let Some(unbonding_list) = unbonding_purses.remove(&validator_public_key) {
                burned_amount += unbonding_list
                    .into_iter()
                    .map(|unbonding_purse| *unbonding_purse.amount())
                    .sum();
                unbonding_purses_modified = true;
            }
        }

        if unbonding_purses_modified {
            detail::set_unbonding_purses(self, unbonding_purses)?;
        }

        if bids_modified {
            detail::set_bids(self, bids)?;
        }

        self.reduce_total_supply(burned_amount)?;

        Ok(())
    }

    /// Takes active_bids and delegators to construct a list of validators' total bids (their own
    /// added to their delegators') ordered by size from largest to smallest, then takes the top N
    /// (number of auction slots) bidders and replaces era_validators with these.
    ///
    /// Accessed by: node
    fn run_auction(&mut self, era_end_timestamp_millis: u64) -> Result<()> {
        if self.get_caller() != SYSTEM_ACCOUNT {
            return Err(Error::InvalidCaller);
        }

        let validator_slots = detail::get_validator_slots(self)?;
        let auction_delay = detail::get_auction_delay(self)?;
        let snapshot_size = auction_delay as usize + 1;
        let mut era_id = detail::get_era_id(self)?;
        let mut bids = detail::get_bids(self)?;

        // Process unbond requests
        detail::process_unbond_requests(self)?;

        // Process bids
        let mut bids_modified = false;
        for bid in bids.values_mut() {
            bids_modified = bid.process(era_end_timestamp_millis)
        }

        // Compute next auction winners
        let winners: ValidatorWeights = {
            let founder_weights: ValidatorWeights = bids
                .iter()
                .filter(|(_public_key, bid)| bid.vesting_schedule().is_some())
                .map(|(public_key, bid)| {
                    let total_staked_amount = bid.total_staked_amount()?;
                    Ok((*public_key, total_staked_amount))
                })
                .collect::<Result<ValidatorWeights>>()?;

            // We collect these into a vec for sorting
            let mut non_founder_weights: Vec<(PublicKey, U512)> = bids
                .iter()
                .filter(|(_public_key, bid)| bid.vesting_schedule().is_none())
                .map(|(public_key, bid)| {
                    let total_staked_amount = bid.total_staked_amount()?;
                    Ok((*public_key, total_staked_amount))
                })
                .collect::<Result<Vec<(PublicKey, U512)>>>()?;

            non_founder_weights.sort_by(|(_, lhs), (_, rhs)| rhs.cmp(lhs));

            let remaining_auction_slots = validator_slots.saturating_sub(founder_weights.len());

            founder_weights
                .into_iter()
                .chain(
                    non_founder_weights
                        .into_iter()
                        .take(remaining_auction_slots),
                )
                .collect()
        };

        // Increment era
        era_id += 1;

        let delayed_era = era_id + auction_delay;

        // Update seigniorage recipients for current era
        {
            let mut snapshot = detail::get_seigniorage_recipients_snapshot(self)?;

            let mut recipients = SeigniorageRecipients::new();

            for era_validator in winners.keys() {
                let seigniorage_recipient = match bids.get(era_validator) {
                    Some(bid) => bid.into(),
                    None => return Err(Error::BidNotFound),
                };
                recipients.insert(*era_validator, seigniorage_recipient);
            }

            let previous_recipients = snapshot.insert(delayed_era, recipients);
            assert!(previous_recipients.is_none());

            let snapshot = snapshot.into_iter().rev().take(snapshot_size).collect();
            detail::set_seigniorage_recipients_snapshot(self, snapshot)?;
        }

        detail::set_era_id(self, era_id)?;
        detail::set_era_end_timestamp_millis(self, era_end_timestamp_millis)?;

        if bids_modified {
            detail::set_bids(self, bids)?;
        }

        Ok(())
    }

    /// Mint and distribute seigniorage rewards to validators and their delegators,
    /// according to `reward_factors` returned by the consensus component.
    fn distribute(&mut self, reward_factors: BTreeMap<PublicKey, u64>) -> Result<()> {
        if self.get_caller() != SYSTEM_ACCOUNT {
            return Err(Error::InvalidCaller);
        }

        let seigniorage_recipients = self.read_seigniorage_recipients()?;
        let base_round_reward = self.read_base_round_reward()?;
        let era_id = detail::get_era_id(self)?;

        if reward_factors.keys().ne(seigniorage_recipients.keys()) {
            return Err(Error::MismatchedEraValidators);
        }

        let mut era_info = EraInfo::new();
        let mut seigniorage_allocations = era_info.seigniorage_allocations_mut();

        let mut bids = detail::get_bids(self)?;

        for (public_key, reward_factor) in reward_factors {
            let recipient = seigniorage_recipients
                .get(&public_key)
                .ok_or(Error::ValidatorNotFound)?;

            let total_stake = recipient.total_stake();
            if total_stake.is_zero() {
                // TODO: error?
                continue;
            }

            let total_reward: Ratio<U512> = {
                let reward_rate = Ratio::new(U512::from(reward_factor), U512::from(BLOCK_REWARD));
                reward_rate * base_round_reward
            };

            let delegator_total_stake: U512 = recipient.delegator_total_stake();

            let delegators_part: Ratio<U512> = {
                let commission_rate = Ratio::new(
                    U512::from(*recipient.delegation_rate()),
                    U512::from(DELEGATION_RATE_DENOMINATOR),
                );
                let reward_multiplier: Ratio<U512> = Ratio::new(delegator_total_stake, total_stake);
                let delegator_reward: Ratio<U512> = total_reward * reward_multiplier;
                let commission: Ratio<U512> = delegator_reward * commission_rate;
                delegator_reward - commission
            };

            let delegator_rewards =
                recipient
                    .delegators()
                    .iter()
                    .map(|(delegator_key, delegator)| {
                        let delegator_stake = delegator.staked_amount();
                        let reward_multiplier = Ratio::new(*delegator_stake, delegator_total_stake);
                        let reward = delegators_part * reward_multiplier;
                        (*delegator_key, reward)
                    });
            let (total_delegator_payout, updated_delegator_rewards) =
                detail::update_delegator_rewards(
                    &mut bids,
                    &mut seigniorage_allocations,
                    public_key,
                    delegator_rewards,
                )?;

            let validators_part: Ratio<U512> = total_reward - Ratio::from(total_delegator_payout);
            let validator_reward = validators_part.to_integer();

            if let Ok(validator_updated_bid) = detail::update_validator_reward(
                &mut bids,
                &mut seigniorage_allocations,
                public_key,
                validator_reward,
            ) {
                // TODO: add "mint into existing purse" facility
                let tmp_validator_reward_purse =
                    self.mint(validator_reward).map_err(|_| Error::MintReward)?;
                self.transfer_purse_to_purse(
                    tmp_validator_reward_purse,
                    *validator_updated_bid.bonding_purse(),
                    validator_reward,
                )
                .map_err(|_| Error::ValidatorRewardTransfer)?;
            }

            debug_assert_eq!(
                updated_delegator_rewards
                    .iter()
                    .map(|(amount, _)| *amount)
                    .sum::<U512>(),
                total_delegator_payout,
            );

            // TODO: add "mint into existing purse" facility
            let tmp_delegator_reward_purse = self
                .mint(total_delegator_payout)
                .map_err(|_| Error::MintReward)?;

            for (reward_amount, bonding_purse) in updated_delegator_rewards {
                self.transfer_purse_to_purse(
                    tmp_delegator_reward_purse,
                    bonding_purse,
                    reward_amount,
                )
                .map_err(|_| Error::DelegatorRewardTransfer)?;
            }
        }

        self.record_era_info(era_id, era_info)?;
        detail::set_bids(self, bids)?;

        Ok(())
    }

    /// Reads current era id.
    fn read_era_id(&mut self) -> Result<EraId> {
        detail::get_era_id(self)
    }
}
