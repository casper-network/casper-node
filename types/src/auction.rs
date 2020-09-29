//! Contains implementation of a Auction contract functionality.
mod bid;
mod constants;
mod detail;
mod era_validators;
mod internal;
mod providers;
mod seigniorage_recipient;
mod types;
mod unbonding_purse;

use alloc::{collections::BTreeMap, vec::Vec};

use num_rational::Ratio;

use crate::{
    system_contract_errors::auction::{Error, Result},
    Key, PublicKey, URef, U512,
};

pub use bid::{Bid, Bids};
pub use constants::*;
pub use era_validators::{EraId, EraValidators, ValidatorWeights};
pub use providers::{MintProvider, RuntimeProvider, StorageProvider, SystemProvider};
pub use seigniorage_recipient::{
    SeigniorageRecipient, SeigniorageRecipients, SeigniorageRecipientsSnapshot,
};
pub use types::*;
pub use unbonding_purse::{UnbondingPurse, UnbondingPurses};

/// Bidders mapped to their bidding purses and tokens contained therein. Delegators' tokens
/// are kept in the validator bid purses, available for withdrawal up to the delegated number
/// of tokens. Withdrawal moves the tokens to a delegator-controlled unbonding purse.
pub type BidPurses = BTreeMap<PublicKey, URef>;

/// Name of bid purses named key.
pub const BID_PURSES_KEY: &str = "bid_purses";
/// Name of unbonding purses key.
pub const UNBONDING_PURSES_KEY: &str = "unbonding_purses";

/// Default number of eras that need to pass to be able to withdraw unbonded funds.
pub const DEFAULT_UNBONDING_DELAY: u64 = 14;

/// Bonding auction contract interface
pub trait Auction:
    StorageProvider + SystemProvider + RuntimeProvider + MintProvider + Sized
{
    /// Returns era_validators.
    ///
    /// Publicly accessible, but intended for periodic use by the PoS contract to update its own
    /// internal data structures recording current and past winners.
    fn read_winners(&mut self) -> Result<EraValidators> {
        internal::get_era_validators(self)
    }

    /// Returns validators in era_validators, mapped to their bids or founding stakes, delegation
    /// rates and lists of delegators together with their delegated quantities from delegators.
    /// This function is publicly accessible, but intended for system use by the PoS contract,
    /// because this data is necessary for distributing seigniorage.
    fn read_seigniorage_recipients(&mut self) -> Result<SeigniorageRecipients> {
        // `era_validators` are assumed to be computed already by calling "run_auction" entrypoint.
        let era_index = internal::get_era_id(self)?;
        let mut seigniorage_recipients_snapshot =
            internal::get_seigniorage_recipients_snapshot(self)?;
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
    ) -> Result<(URef, U512)> {
        // Creates new purse with desired amount taken from `source_purse`
        // Bonds whole amount from the newly created purse
        let (bonding_purse, _total_amount) = self.bond(public_key, source, amount)?;

        // Update bids or stakes
        let mut validators = internal::get_bids(self)?;

        let bid = validators
            .entry(public_key)
            .and_modify(|bid| {
                // Update `bids` map since `account_hash` belongs to a validator.
                bid.bonding_purse = bonding_purse;
                bid.delegation_rate = delegation_rate;
                bid.staked_amount += amount;

                // bid.staked_amount
            })
            .or_insert_with(|| {
                // Create new entry.
                Bid {
                    bonding_purse,
                    staked_amount: amount,
                    delegation_rate,
                    funds_locked: None,
                }
            });
        let new_amount = bid.staked_amount;
        internal::set_bids(self, validators)?;

        Ok((bonding_purse, new_amount))
    }

    /// For a non-founder validator, implements essentially the same logic as add_bid, but reducing
    /// the number of tokens and calling unbond in lieu of bond.
    ///
    /// For a founding validator, this function first checks whether they are released, and fails
    /// if they are not.
    ///
    /// The function returns a tuple of the (new) unbonding purse key and the new amount of motes
    /// remaining in the bid. If the target bid does not exist, the function call returns an error.
    fn withdraw_bid(&mut self, public_key: PublicKey, amount: U512) -> Result<(URef, U512)> {
        // Update bids or stakes
        let mut bids = internal::get_bids(self)?;

        let bid = bids.get_mut(&public_key).ok_or(Error::ValidatorNotFound)?;

        let new_amount = if bid.can_withdraw_funds() {
            // Carefully decrease bonded funds
            bid.staked_amount
                .checked_sub(amount)
                .ok_or(Error::InvalidAmount)?
        } else {
            // If validator is still locked-up (or with an autowin status), no withdrawals
            // are allowed.
            return Err(Error::ValidatorFundsLocked);
        };

        internal::set_bids(self, bids)?;

        let (unbonding_purse, _total_amount) = self.unbond(public_key, amount)?;

        Ok((unbonding_purse, new_amount))
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
    ) -> Result<(URef, U512)> {
        let bids = internal::get_bids(self)?;
        if !bids.contains_key(&validator_public_key) {
            // Return early if target validator is not in `bids`
            return Err(Error::ValidatorNotFound);
        }

        let (bonding_purse, _total_amount) = self.bond(delegator_public_key, source, amount)?;

        let new_delegation_amount =
            detail::update_delegators(self, validator_public_key, delegator_public_key, amount)?;

        // Initialize delegator_reward_pool_map entry if it doesn't exist.
        {
            let mut delegator_reward_map = internal::get_delegator_reward_map(self)?;
            delegator_reward_map
                .entry(validator_public_key)
                .or_default()
                .entry(delegator_public_key)
                .or_insert_with(U512::zero);
            internal::set_delegator_reward_map(self, delegator_reward_map)?;
        }

        Ok((bonding_purse, new_delegation_amount))
    }

    /// Removes an amount of motes (or the entry altogether, if the remaining amount is 0) from
    /// the entry in delegators and calls unbond in the Mint contract to create a new unbonding
    /// purse.
    ///
    /// The arguments are the delegator’s key, the validator key and quantity of motes.
    /// The return value is the remaining bond after this quantity is subtracted.
    fn undelegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Result<U512> {
        let bids = internal::get_bids(self)?;

        // Return early if target validator is not in `bids`
        if !bids.contains_key(&validator_public_key) {
            return Err(Error::ValidatorNotFound);
        }

        let (_unbonding_purse, _total_amount) = self.unbond(delegator_public_key, amount)?;

        let mut delegators = internal::get_delegators(self)?;
        let delegators_map = delegators
            .get_mut(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let new_amount = {
            let delegators_amount = delegators_map
                .get_mut(&delegator_public_key)
                .ok_or(Error::DelegatorNotFound)?;

            let new_amount = delegators_amount
                .checked_sub(amount)
                .ok_or(Error::InvalidAmount)?;

            *delegators_amount = new_amount;
            new_amount
        };

        if new_amount.is_zero() {
            // Inner map's mapped value should be zero as we subtracted mutable value.
            let _value = delegators_map
                .remove(&validator_public_key)
                .ok_or(Error::ValidatorNotFound)?;
            debug_assert!(_value.is_zero());

            let mut outer = internal::get_delegator_reward_map(self)?;
            let mut inner = outer
                .remove(&validator_public_key)
                .ok_or(Error::ValidatorNotFound)?;
            inner
                .remove(&delegator_public_key)
                .ok_or(Error::DelegatorNotFound)?;
            if !inner.is_empty() {
                outer.insert(validator_public_key, inner);
            };
            internal::set_delegator_reward_map(self, outer)?;
        }

        internal::set_delegators(self, delegators)?;

        Ok(new_amount)
    }

    /// Removes validator entries from either founders or validators, wherever they
    /// might be found.
    ///
    /// This function is intended to be called together with the slash function in the Mint
    /// contract.
    fn quash_bid(&mut self, validator_public_keys: Vec<PublicKey>) -> Result<()> {
        // Clean up inside `bids`
        let mut validators = internal::get_bids(self)?;

        let mut modified_validators = 0usize;

        for validator_public_key in &validator_public_keys {
            if validators.remove(validator_public_key).is_some() {
                modified_validators += 1;
            }
        }

        if modified_validators > 0 {
            internal::set_bids(self, validators)?;
        }

        Ok(())
    }

    /// Creates a new purse in bid_purses corresponding to a validator's key, or tops off an
    /// existing one.
    ///
    /// Returns the bid purse's key and current amount of motes.
    fn bond(&mut self, public_key: PublicKey, source: URef, amount: U512) -> Result<(URef, U512)> {
        if amount.is_zero() {
            return Err(Error::BondTooSmall);
        }

        let bid_purses_uref = self
            .get_key(BID_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;

        let mut bid_purses: BidPurses = self.read(bid_purses_uref)?.ok_or(Error::Storage)?;

        let target = match bid_purses.get(&public_key) {
            Some(purse) => *purse,
            None => {
                let new_purse = self.create_purse();
                bid_purses.insert(public_key, new_purse);
                self.write(bid_purses_uref, bid_purses)?;
                new_purse
            }
        };

        self.transfer_from_purse_to_purse(source, target, amount)?;

        let total_amount = self.get_balance(target)?.unwrap();

        Ok((target, total_amount))
    }

    /// Creates a new purse in unbonding_purses given a validator's key and amount, returning
    /// the new purse's key and the amount of motes remaining in the validator's bid purse.
    fn unbond(&mut self, public_key: PublicKey, amount: U512) -> Result<(URef, U512)> {
        let bid_purses_uref = self
            .get_key(BID_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;

        let bid_purses: BidPurses = self.read(bid_purses_uref)?.ok_or(Error::Storage)?;

        let bid_purse = bid_purses
            .get(&public_key)
            .copied()
            .ok_or(Error::BondNotFound)?;

        if self.get_balance(bid_purse)?.unwrap_or_default() < amount {
            return Err(Error::UnbondTooLarge);
        }

        // Creates new unbonding purse with requested tokens
        let unbond_purse = self.create_purse();

        // Update `unbonding_purses` data
        let unbonding_purses_uref = self
            .get_key(UNBONDING_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;
        let mut unbonding_purses: UnbondingPurses =
            self.read(unbonding_purses_uref)?.ok_or(Error::Storage)?;

        let current_era_id = self.read_era_id()?;
        let new_unbonding_purse = UnbondingPurse {
            purse: unbond_purse,
            origin: public_key,
            era_of_withdrawal: current_era_id + DEFAULT_UNBONDING_DELAY,
            amount,
        };
        unbonding_purses
            .entry(public_key)
            .or_default()
            .push(new_unbonding_purse);
        self.write(unbonding_purses_uref, unbonding_purses)?;

        // Remaining motes in the validator's bid purse
        let remaining_bond = self.get_balance(bid_purse)?.unwrap_or_default();
        Ok((unbond_purse, remaining_bond))
    }

    /// Slashes each validator.
    ///
    /// This can be only invoked through a system call.
    fn slash(&mut self, validator_public_keys: Vec<PublicKey>) -> Result<()> {
        if self.get_caller() != SYSTEM_ACCOUNT {
            return Err(Error::InvalidCaller);
        }

        let bid_purses_uref = self
            .get_key(BID_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;

        let mut bid_purses: BidPurses = self.read(bid_purses_uref)?.ok_or(Error::Storage)?;

        let unbonding_purses_uref = self
            .get_key(UNBONDING_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;
        let mut unbonding_purses: UnbondingPurses =
            self.read(unbonding_purses_uref)?.ok_or(Error::Storage)?;

        let mut bid_purses_modified = false;
        let mut unbonding_purses_modified = false;
        for validator_account_hash in validator_public_keys {
            if let Some(_bid_purse) = bid_purses.remove(&validator_account_hash) {
                bid_purses_modified = true;
            }

            if let Some(unbonding_list) = unbonding_purses.get_mut(&validator_account_hash) {
                let size_before = unbonding_list.len();

                unbonding_list.retain(|element| element.origin != validator_account_hash);

                unbonding_purses_modified = size_before != unbonding_list.len();
            }
        }

        if bid_purses_modified {
            self.write(bid_purses_uref, bid_purses)?;
        }

        if unbonding_purses_modified {
            self.write(unbonding_purses_uref, unbonding_purses)?;
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
            return Err(Error::InvalidContext);
        }

        detail::process_unbond_requests(self)?;

        let mut era_id = internal::get_era_id(self)?;

        let mut bids = internal::get_bids(self)?;
        //
        // Process locked bids
        //
        let mut bids_modified = false;
        for bid in bids.values_mut() {
            if let Some(locked_until) = bid.funds_locked {
                if era_id >= locked_until {
                    bid.funds_locked = None;
                    bids_modified = true;
                }
            }
        }

        //
        // Compute next auction slots
        //

        // Take winning validators and add them to validator_weights right away.
        let mut bid_weights: ValidatorWeights = {
            bids.iter()
                .filter(|(_validator_account_hash, founding_validator)| {
                    founding_validator.funds_locked.is_some()
                })
                .map(|(validator_account_hash, amount)| {
                    (*validator_account_hash, amount.staked_amount)
                })
                .collect()
        };

        // Non-winning validators are taken care of later
        let bid_scores = bids
            .iter()
            .filter(|(_validator_account_hash, founding_validator)| {
                founding_validator.funds_locked.is_none()
            })
            .map(|(validator_account_hash, amount)| {
                (*validator_account_hash, amount.staked_amount)
            });

        // Validator's entries from both maps as a single iterable.
        // let all_scores = founders_scores.chain(validators_scores);

        // All the scores are then grouped by the account hash to calculate a sum of each
        // consecutive scores for each validator.
        let mut scores = BTreeMap::new();
        for (account_hash, score) in bid_scores {
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
        let remaining_auction_slots = AUCTION_SLOTS.saturating_sub(bid_weights.len());
        bid_weights.extend(scores.into_iter().take(remaining_auction_slots));

        let mut era_validators = internal::get_era_validators(self)?;

        // Era index is assumed to be equal to era id on the consensus side.
        era_id += 1;

        let next_era_id = era_id + AUCTION_DELAY;

        //
        // Compute seiginiorage recipients for current era
        //
        let mut delegators = internal::get_delegators(self)?;
        let mut seigniorage_recipients_snapshot =
            internal::get_seigniorage_recipients_snapshot(self)?;
        let mut seigniorage_recipients = SeigniorageRecipients::new();

        // for each validator...
        for era_validator in bid_weights.keys() {
            let mut seigniorage_recipient = SeigniorageRecipient::default();
            // ... mapped to their bids
            if let Some(founding_validator) = bids.get(era_validator) {
                seigniorage_recipient.stake = founding_validator.staked_amount;
                seigniorage_recipient.delegation_rate = founding_validator.delegation_rate;
            }

            if let Some(delegator_map) = delegators.remove(era_validator) {
                seigniorage_recipient.delegators = delegator_map;
            }

            seigniorage_recipients.insert(*era_validator, seigniorage_recipient);
        }
        let previous_seigniorage_recipients =
            seigniorage_recipients_snapshot.insert(next_era_id, seigniorage_recipients);
        assert!(previous_seigniorage_recipients.is_none());

        let seigniorage_recipients_snapshot = seigniorage_recipients_snapshot
            .into_iter()
            .rev()
            .take(SNAPSHOT_SIZE)
            .collect();
        internal::set_seigniorage_recipients_snapshot(self, seigniorage_recipients_snapshot)?;

        // Index for next set of validators: `era_id + AUCTION_DELAY`
        let previous_era_validators = era_validators.insert(era_id + AUCTION_DELAY, bid_weights);
        assert!(previous_era_validators.is_none());

        internal::set_era_id(self, era_id)?;

        // Keep maximum of `AUCTION_DELAY + 1` elements
        let era_validators = era_validators
            .into_iter()
            .rev()
            .take(SNAPSHOT_SIZE)
            .collect();

        internal::set_era_validators(self, era_validators)?;

        if bids_modified {
            internal::set_bids(self, bids)?;
        }

        Ok(())
    }

    /// Mint and distribute seigniorage rewards to validators and their delegators,
    /// according to `reward_factors` returned by the consensus component.
    fn distribute(&mut self, reward_factors: BTreeMap<PublicKey, u64>) -> Result<()> {
        if self.get_caller() != SYSTEM_ACCOUNT {
            return Err(Error::InvalidContext);
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
                    .map(|(delegator_key, delegator_stake)| {
                        let reward_multiplier = Ratio::new(*delegator_stake, delegator_total_stake);
                        let reward = delegators_part * reward_multiplier;
                        (*delegator_key, reward)
                    });
            let remainder: Ratio<U512> =
                detail::update_delegator_rewards(self, public_key, delegator_rewards)?;

            let validators_part: Ratio<U512> = total_reward - delegators_part + remainder;
            let validator_reward = validators_part.to_integer();
            detail::update_validator_reward(self, public_key, validator_reward)?;

            // TODO: add "mint into existing purse" facility
            let validator_reward_purse = self
                .get_key(VALIDATOR_REWARD_PURSE)
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

            let delegators_reward: U512 = (delegators_part - remainder).ceil().to_integer();

            // TODO: add "mint into existing purse" facility
            let delegator_reward_purse = self
                .get_key(DELEGATOR_REWARD_PURSE)
                .ok_or(Error::MissingKey)?
                .into_uref()
                .ok_or(Error::InvalidKeyVariant)?;
            let tmp_delegator_reward_purse = self
                .mint(delegators_reward)
                .map_err(|_| Error::MintReward)?;
            self.transfer_purse_to_purse(
                tmp_delegator_reward_purse,
                delegator_reward_purse,
                delegators_reward,
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
        let mut outer: DelegatorRewardMap = internal::get_delegator_reward_map(self)?;
        let mut inner = outer
            .remove(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let reward_amount: &mut U512 = inner
            .get_mut(&delegator_public_key)
            .ok_or(Error::DelegatorNotFound)?;

        let ret = *reward_amount;

        if !ret.is_zero() {
            let source_purse = self
                .get_key(DELEGATOR_REWARD_PURSE)
                .ok_or(Error::MissingKey)?
                .into_uref()
                .ok_or(Error::InvalidKeyVariant)?;

            self.transfer_purse_to_purse(source_purse, target_purse, *reward_amount)
                .map_err(|_| Error::Transfer)?;

            *reward_amount = U512::zero();
        }

        outer.insert(validator_public_key, inner);
        internal::set_delegator_reward_map(self, outer)?;
        Ok(ret)
    }

    /// Allows validators to withdraw the seigniorage rewards they have earned.
    /// Pays out the entire accumulated amount to the destination purse.
    fn withdraw_validator_reward(
        &mut self,
        validator_public_key: PublicKey,
        target_purse: URef,
    ) -> Result<U512> {
        let mut validator_reward_map = internal::get_validator_reward_map(self)?;

        let reward_amount: &mut U512 = validator_reward_map
            .get_mut(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let ret = *reward_amount;

        if !ret.is_zero() {
            let source_purse = self
                .get_key(VALIDATOR_REWARD_PURSE)
                .ok_or(Error::MissingKey)?
                .into_uref()
                .ok_or(Error::InvalidKeyVariant)?;

            self.transfer_purse_to_purse(source_purse, target_purse, *reward_amount)
                .map_err(|_| Error::Transfer)?;

            *reward_amount = U512::zero();
        }

        internal::set_validator_reward_map(self, validator_reward_map)?;
        Ok(ret)
    }

    /// Reads current era id.
    fn read_era_id(&mut self) -> Result<EraId> {
        internal::get_era_id(self)
    }
}
