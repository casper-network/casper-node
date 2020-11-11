//! Contains implementation of a Auction contract functionality.
mod bid;
mod constants;
mod delegator;
mod detail;
mod providers;
mod seigniorage_recipient;
mod types;
mod unbonding_purse;

use alloc::{collections::BTreeMap, vec::Vec};

use num_rational::Ratio;

use crate::{
    account::AccountHash,
    system_contract_errors::auction::{Error, Result},
    PublicKey, URef, U512,
};

pub use bid::Bid;
pub use constants::*;
pub use delegator::Delegator;
pub use providers::{MintProvider, RuntimeProvider, StorageProvider, SystemProvider};
pub use seigniorage_recipient::SeigniorageRecipient;
pub use types::*;
pub use unbonding_purse::UnbondingPurse;

/// Bonding auction contract interface
pub trait Auction:
    StorageProvider + SystemProvider + RuntimeProvider + MintProvider + Sized
{
    /// Returns era_validators.
    ///
    /// Publicly accessible, but intended for periodic use by the PoS contract to update its own
    /// internal data structures recording current and past winners.
    fn get_era_validators(&mut self) -> Result<EraValidators> {
        let era_validators = detail::get_era_validators(self)?;
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
            .unwrap_or_else(|| panic!("No seigniorage_recipients for era {}", era_index));
        Ok(seigniorage_recipients)
    }

    /// For a non-founder validator, this adds, or modifies, an entry in the `bids` collection and
    /// calls `bond` in the Mint contract to create (or top off) a bid purse. It also adjusts the
    /// delegation rate.
    fn add_bid(
        &mut self,
        public_key: PublicKey,
        source: URef,
        delegation_rate: DelegationRate,
        amount: U512,
    ) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(public_key, |x| self.blake2b(x));
        if self.get_caller() != account_hash {
            return Err(Error::InvalidPublicKey);
        }

        if amount.is_zero() {
            return Err(Error::BondTooSmall);
        }

        // Update bids or stakes
        let mut validators = detail::get_bids(self)?;
        let new_amount = match validators.get_mut(&public_key) {
            Some(bid) => {
                self.transfer_purse_to_purse(source, *bid.bonding_purse(), amount)?;
                bid.with_delegation_rate(delegation_rate)
                    .increase_stake(amount)?
            }
            None => {
                let bonding_purse = self.create_purse();
                self.transfer_purse_to_purse(source, bonding_purse, amount)?;
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
    fn withdraw_bid(
        &mut self,
        public_key: PublicKey,
        amount: U512,
        unbonding_purse: URef,
    ) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(public_key, |x| self.blake2b(x));
        if self.get_caller() != account_hash {
            return Err(Error::InvalidPublicKey);
        }

        // Update bids or stakes
        let mut bids = detail::get_bids(self)?;

        let bid = bids.get_mut(&public_key).ok_or(Error::ValidatorNotFound)?;

        detail::create_unbonding_purse(
            self,
            public_key,
            *bid.bonding_purse(),
            unbonding_purse,
            amount,
        )?;

        let new_amount = bid.decrease_stake(amount)?;

        if new_amount.is_zero() {
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
        source: URef,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(delegator_public_key, |x| self.blake2b(x));
        if self.get_caller() != account_hash {
            return Err(Error::InvalidPublicKey);
        }

        if amount.is_zero() {
            return Err(Error::BondTooSmall);
        }

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
                self.transfer_purse_to_purse(source, *delegator.bonding_purse(), amount)?;
                delegator.increase_stake(amount)?;
                *delegator.staked_amount()
            }
            None => {
                let bonding_purse = self.create_purse();
                self.transfer_purse_to_purse(source, bonding_purse, amount)?;
                let delegator = Delegator::new(amount, bonding_purse, validator_public_key);
                delegators.insert(delegator_public_key, delegator);
                amount
            }
        };

        detail::set_bids(self, bids)?;

        // Initialize delegator_reward_pool_map entry if it doesn't exist.
        {
            let mut delegator_reward_map = detail::get_delegator_reward_map(self)?;
            delegator_reward_map
                .entry(validator_public_key)
                .or_default()
                .entry(delegator_public_key)
                .or_insert_with(U512::zero);
            detail::set_delegator_reward_map(self, delegator_reward_map)?;
        }

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
        unbonding_purse: URef,
    ) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(delegator_public_key, |x| self.blake2b(x));
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
                    delegator_public_key,
                    *delegator.bonding_purse(),
                    unbonding_purse,
                    amount,
                )?;
                let updated_stake = delegator.decrease_stake(amount)?;
                if updated_stake == U512::zero() {
                    delegators.remove(&delegator_public_key);
                };
                updated_stake
            }
            None => {
                return Err(Error::DelegatorNotFound);
            }
        };

        detail::set_bids(self, bids)?;

        if new_amount.is_zero() {
            let mut outer = detail::get_delegator_reward_map(self)?;
            let mut inner = outer
                .remove(&validator_public_key)
                .ok_or(Error::ValidatorNotFound)?;
            inner
                .remove(&delegator_public_key)
                .ok_or(Error::DelegatorNotFound)?;
            if !inner.is_empty() {
                outer.insert(validator_public_key, inner);
            };
            detail::set_delegator_reward_map(self, outer)?;
        }

        Ok(new_amount)
    }

    /// Slashes each validator.
    ///
    /// This can be only invoked through a system call.
    fn slash(&mut self, validator_public_keys: Vec<PublicKey>) -> Result<()> {
        if self.get_caller() != SYSTEM_ACCOUNT {
            return Err(Error::InvalidCaller);
        }

        detail::quash_bid(self, &validator_public_keys)?;

        let mut unbonding_purses: UnbondingPurses = detail::get_unbonding_purses(self)?;

        let mut unbonding_purses_modified = false;
        for validator_public_key in validator_public_keys {
            if let Some(unbonding_list) = unbonding_purses.get_mut(&validator_public_key) {
                let size_before = unbonding_list.len();

                unbonding_list.retain(|element| element.public_key != validator_public_key);

                unbonding_purses_modified = size_before != unbonding_list.len();
            }
        }

        if unbonding_purses_modified {
            detail::set_unbonding_purses(self, unbonding_purses)?;
        }

        Ok(())
    }

    /// Takes active_bids and delegators to construct a list of validators' total bids (their own
    /// added to their delegators') ordered by size from largest to smallest, then takes the top N
    /// (number of auction slots) bidders and replaces era_validators with these.
    ///
    /// Accessed by: node
    fn run_auction(&mut self) -> Result<()> {
        if self.get_caller() != SYSTEM_ACCOUNT {
            return Err(Error::InvalidCaller);
        }

        detail::process_unbond_requests(self)?;

        // get allowed validator slots total
        let validator_slots = detail::get_validator_slots(self)?;

        let auction_delay = detail::get_auction_delay(self)?;
        let snapshot_size = auction_delay as usize + 1;

        let mut era_id = detail::get_era_id(self)?;

        let mut bids = detail::get_bids(self)?;

        //
        // Process locked bids
        //
        let mut bids_modified = false;
        for bid in bids.values_mut() {
            if bid.unlock(era_id) {
                bids_modified = true;
            }
        }

        //
        // Compute next auction slots
        //

        // Take winning validators and add them to validator_weights right away.
        let mut bid_weights: ValidatorWeights = bids
            .iter()
            .filter(|(_validator_account_hash, founding_validator)| founding_validator.is_locked())
            .map(|(validator_account_hash, amount)| {
                let total_staked_amount = amount.total_staked_amount()?;
                Ok((*validator_account_hash, total_staked_amount))
            })
            .collect::<Result<ValidatorWeights>>()?;

        // Non-winning validators are taken care of later
        let bid_scores = bids
            .iter()
            .filter(|(_validator_account_hash, founding_validator)| !founding_validator.is_locked())
            .map(|(validator_account_hash, amount)| {
                let total_staked_amount = amount.total_staked_amount()?;
                Ok((*validator_account_hash, total_staked_amount))
            })
            .collect::<Result<ValidatorWeights>>()?;

        // Validator's entries from both maps as a single iterable.
        // let all_scores = founders_scores.chain(validators_scores);

        // All the scores are then grouped by the account hash to calculate a sum of each
        // consecutive scores for each validator.
        let mut scores = BTreeMap::new();
        for (account_hash, score) in bid_scores.into_iter() {
            scores
                .entry(account_hash)
                .and_modify(|acc| *acc += score)
                .or_insert_with(|| score);
        }

        // Compute new winning validators.
        let mut scores: Vec<_> = scores.into_iter().collect();
        // Sort the results in descending order
        scores.sort_by(|(_, lhs), (_, rhs)| rhs.cmp(lhs));

        // Fill in remaining validators
        let remaining_auction_slots = validator_slots.saturating_sub(bid_weights.len());
        bid_weights.extend(scores.into_iter().take(remaining_auction_slots));

        let mut era_validators = detail::get_era_validators(self)?;

        // Era index is assumed to be equal to era id on the consensus side.
        era_id += 1;

        let next_era_id = era_id + auction_delay;

        //
        // Compute seiginiorage recipients for current era
        //
        let mut seigniorage_recipients_snapshot =
            detail::get_seigniorage_recipients_snapshot(self)?;
        let mut seigniorage_recipients = SeigniorageRecipients::new();

        // for each validator...
        for era_validator in bid_weights.keys() {
            let mut seigniorage_recipient = SeigniorageRecipient::default();
            // ... mapped to their bids
            if let Some(founding_validator) = bids.get(era_validator) {
                seigniorage_recipient.stake = *founding_validator.staked_amount();
                seigniorage_recipient.delegation_rate = *founding_validator.delegation_rate();
            }

            if let Some(bid) = bids.get(&era_validator) {
                seigniorage_recipient.delegators = bid.delegators().clone();
            }

            seigniorage_recipients.insert(*era_validator, seigniorage_recipient);
        }
        let previous_seigniorage_recipients =
            seigniorage_recipients_snapshot.insert(next_era_id, seigniorage_recipients);
        assert!(previous_seigniorage_recipients.is_none());

        let seigniorage_recipients_snapshot = seigniorage_recipients_snapshot
            .into_iter()
            .rev()
            .take(snapshot_size)
            .collect();
        detail::set_seigniorage_recipients_snapshot(self, seigniorage_recipients_snapshot)?;

        // Index for next set of validators: `era_id + AUCTION_DELAY`
        let previous_era_validators = era_validators.insert(era_id + auction_delay, bid_weights);
        assert!(previous_era_validators.is_none());

        detail::set_era_id(self, era_id)?;
        // Keep maximum of `AUCTION_DELAY + 1` elements
        let era_validators = era_validators
            .into_iter()
            .rev()
            .take(snapshot_size)
            .collect();

        detail::set_era_validators(self, era_validators)?;

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

        if reward_factors.keys().ne(seigniorage_recipients.keys()) {
            return Err(Error::MismatchedEraValidators);
        }

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
                    U512::from(recipient.delegation_rate),
                    U512::from(DELEGATION_RATE_DENOMINATOR),
                );
                let reward_multiplier: Ratio<U512> = Ratio::new(delegator_total_stake, total_stake);
                let delegator_reward: Ratio<U512> = total_reward * reward_multiplier;
                let commission: Ratio<U512> = delegator_reward * commission_rate;
                delegator_reward - commission
            };

            let delegator_rewards =
                recipient
                    .delegators
                    .iter()
                    .map(|(delegator_key, delegator)| {
                        let delegator_stake = delegator.staked_amount();
                        let reward_multiplier = Ratio::new(*delegator_stake, delegator_total_stake);
                        let reward = delegators_part * reward_multiplier;
                        (*delegator_key, reward)
                    });
            let total_delegator_payout: U512 =
                detail::update_delegator_rewards(self, public_key, delegator_rewards)?;

            let validators_part: Ratio<U512> = total_reward - Ratio::from(total_delegator_payout);
            let validator_reward = validators_part.to_integer();
            detail::update_validator_reward(self, public_key, validator_reward)?;

            // TODO: add "mint into existing purse" facility
            let validator_reward_purse = self
                .get_key(VALIDATOR_REWARD_PURSE_KEY)
                .ok_or(Error::MissingKey)?
                .into_uref()
                .ok_or(Error::InvalidKeyVariant)?;
            let tmp_validator_reward_purse =
                self.mint(validator_reward).map_err(|_| Error::MintReward)?;
            self.transfer_purse_to_purse(
                tmp_validator_reward_purse,
                validator_reward_purse,
                validator_reward,
            )
            .map_err(|_| Error::Transfer)?;

            // TODO: add "mint into existing purse" facility
            let delegator_reward_purse = self
                .get_key(DELEGATOR_REWARD_PURSE_KEY)
                .ok_or(Error::MissingKey)?
                .into_uref()
                .ok_or(Error::InvalidKeyVariant)?;
            let tmp_delegator_reward_purse = self
                .mint(total_delegator_payout)
                .map_err(|_| Error::MintReward)?;
            self.transfer_purse_to_purse(
                tmp_delegator_reward_purse,
                delegator_reward_purse,
                total_delegator_payout,
            )
            .map_err(|_| Error::Transfer)?;
        }
        Ok(())
    }

    /// Allows delegators to withdraw the seigniorage rewards they have earned.
    /// Pays out the entire accumulated amount to the destination purse.
    fn withdraw_delegator_reward(
        &mut self,
        validator_public_key: PublicKey,
        delegator_public_key: PublicKey,
        target_purse: URef,
    ) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(delegator_public_key, |x| self.blake2b(x));
        if self.get_caller() != account_hash {
            return Err(Error::InvalidPublicKey);
        }

        let mut outer: DelegatorRewardMap = detail::get_delegator_reward_map(self)?;
        let mut inner = outer
            .remove(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let reward_amount: &mut U512 = inner
            .get_mut(&delegator_public_key)
            .ok_or(Error::DelegatorNotFound)?;

        let ret = *reward_amount;

        if !ret.is_zero() {
            let source_purse = self
                .get_key(DELEGATOR_REWARD_PURSE_KEY)
                .ok_or(Error::MissingKey)?
                .into_uref()
                .ok_or(Error::InvalidKeyVariant)?;

            self.transfer_purse_to_purse(source_purse, target_purse, *reward_amount)
                .map_err(|_| Error::Transfer)?;

            *reward_amount = U512::zero();
        }

        outer.insert(validator_public_key, inner);
        detail::set_delegator_reward_map(self, outer)?;
        Ok(ret)
    }

    /// Allows validators to withdraw the seigniorage rewards they have earned.
    /// Pays out the entire accumulated amount to the destination purse.
    fn withdraw_validator_reward(
        &mut self,
        validator_public_key: PublicKey,
        target_purse: URef,
    ) -> Result<U512> {
        let account_hash = AccountHash::from_public_key(validator_public_key, |x| self.blake2b(x));
        if self.get_caller() != account_hash {
            return Err(Error::InvalidPublicKey);
        }

        let mut validator_reward_map = detail::get_validator_reward_map(self)?;

        let reward_amount: &mut U512 = validator_reward_map
            .get_mut(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let ret = *reward_amount;

        if !ret.is_zero() {
            let source_purse = self
                .get_key(VALIDATOR_REWARD_PURSE_KEY)
                .ok_or(Error::MissingKey)?
                .into_uref()
                .ok_or(Error::InvalidKeyVariant)?;

            self.transfer_purse_to_purse(source_purse, target_purse, *reward_amount)
                .map_err(|_| Error::Transfer)?;

            *reward_amount = U512::zero();
        }

        detail::set_validator_reward_map(self, validator_reward_map)?;
        Ok(ret)
    }

    /// Reads current era id.
    fn read_era_id(&mut self) -> Result<EraId> {
        detail::get_era_id(self)
    }
}
