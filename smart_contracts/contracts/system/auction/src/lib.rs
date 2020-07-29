#![cfg_attr(not(test), no_std)]

#[macro_use]
extern crate alloc;

mod entry_points;
mod error;
mod internal;
mod providers;

use alloc::{collections::BinaryHeap, vec::Vec};

use casperlabs_types::{
    account::AccountHash,
    auction::{ActiveBid, DelegationRate, SeigniorageRecipients},
    URef, U512,
};
use itertools::Itertools;

pub use entry_points::get_entry_points;
pub use error::{Error, Result};
use providers::{ProofOfStakeProvider, StorageProvider, SystemProvider};

const AUCTION_SLOTS: usize = 5;

pub trait Auction: StorageProvider + ProofOfStakeProvider + SystemProvider
where
    Error: From<<Self as StorageProvider>::Error> + From<<Self as SystemProvider>::Error>,
{
    /// Access: node
    /// Upon progression to the set era marking the release of founding stakes,
    /// node software embeds a deploy in the first block of the relevant era
    /// to trigger this function. The founding_validators data structure is
    /// checked, returning False if the validator is not found and aborting.
    /// If the validator is found, the function first calls
    /// release_founder_stake in the Mint contract. Upon receipt of True,
    /// it also flips the relevant field to True in the validator’s entry
    /// within founding_validators. Otherwise the function aborts.
    fn release_founder(&mut self, _account_hash: AccountHash) -> bool {
        todo!()
    }

    /// Returns era_validators. Publicly accessible, but intended
    /// for periodic use by the PoS contract to update its own internal data
    /// structures recording current and past winners.
    fn read_winners(&mut self) -> Result<Vec<AccountHash>> {
        internal::get_era_validators(self)
    }

    /// Returns validators in era_validators, mapped to their bids or founding
    /// stakes, delegation rates and lists of delegators together with their
    /// delegated quantities from delegators. This function is publicly
    /// accessible, but intended for system use by the PoS contract, because
    /// this data is necessary for distributing seigniorage.
    fn read_seigniorage_recipients(&mut self) -> SeigniorageRecipients {
        todo!()
    }

    /// For a non-founder validator, this adds, or modifies, an entry in the
    /// active_bids map and calls bond in the Mint contract to create (or top off)
    /// a bid purse. It also adjusts the delegation rate.
    /// For a founding validator, the same logic is carried out with
    /// founding_validators, instead of active_bids.
    /// The arguments, in order, are public key, originating purse, delegation
    /// rate and quantity of motes to add. The function returns a tuple of the
    /// bid (or stake) purse key and the new quantity of motes.
    fn add_bid(
        &mut self,
        account_hash: AccountHash,
        source_purse: URef,
        delegation_rate: DelegationRate,
        quantity: U512,
    ) -> Result<(URef, U512)> {
        // Creates new purse with desired amount taken from `source_purse`
        let bonding_purse = self.create_purse();
        self.transfer_from_purse_to_purse(source_purse, bonding_purse, quantity)?;

        // Update bids or stakes
        let mut founder_validators = internal::get_founder_validators(self)?;

        let new_quantity = match founder_validators.get_mut(&account_hash) {
            // Update `founder_validators` map since `account_hash` belongs to a validator.
            Some(founding_validator) => {
                founding_validator.bonding_purse = bonding_purse;
                founding_validator.staked_amount += quantity;

                founding_validator.staked_amount
            }
            None => {
                // Non-founder - updates `active_bids` table
                let mut active_bids = internal::get_active_bids(self)?;
                // Returns active bid which could be updated in case given entry exists.
                let bid_amount = {
                    let active_bid = active_bids
                        .entry(account_hash)
                        .and_modify(|active_bid| {
                            // Update existing entry
                            active_bid.bid_amount += quantity;
                            active_bid.bid_purse = bonding_purse;
                            active_bid.delegation_rate = delegation_rate;
                        })
                        .or_insert_with(|| {
                            // Create new entry in active bids
                            ActiveBid {
                                bid_purse: bonding_purse,
                                bid_amount: quantity,
                                delegation_rate,
                            }
                        });
                    active_bid.bid_amount
                };

                // Write updated active bids
                internal::set_active_bids(self, active_bids)?;

                bid_amount
            }
        };

        // Bonds whole amount from the newly created purse
        self.bond(quantity, bonding_purse);

        Ok((bonding_purse, new_quantity))
    }

    /// For a non-founder validator, implements essentially the same logic as
    /// add_bid, but reducing the number of tokens and calling unbond in lieu of
    /// bond.
    ///
    /// For a founding validator, this function first checks whether they are
    /// released, and fails if they are not. Additionally, the relevant data
    /// structure is founding_validators, rather than active_bids.
    ///
    /// The function returns a tuple of the (new) unbonding purse key and the new
    /// quantity of motes remaining in the bid. If the target bid does not exist,
    /// The arguments are the public key and amount of motes to remove.
    fn withdraw_bid(&mut self, account_hash: AccountHash, quantity: U512) -> Result<(URef, U512)> {
        // Update bids or stakes
        let mut founder_validators = internal::get_founder_validators(self)?;

        let (bonding_purse, new_quantity) = match founder_validators.get_mut(&account_hash) {
            Some(founding_validator) => {
                // Carefully decrease bonded funds
                let (bonding_purse, new_quantity) =
                    match founding_validator.staked_amount.checked_sub(quantity) {
                        Some(new_staked_amount) => {
                            (founding_validator.bonding_purse, new_staked_amount)
                        }
                        None => {
                            // Decreasing passed quantity would result in negative stake
                            return Err(Error::InvalidQuantity);
                        }
                    };

                internal::set_founder_validators(self, founder_validators)?;

                (bonding_purse, new_quantity)
            }
            None => {
                let mut active_bids = internal::get_active_bids(self)?;

                let (bid_purse, new_amount) = match active_bids.get_mut(&account_hash) {
                    Some(active_bid) => {
                        let new_amount = active_bid
                            .bid_amount
                            .checked_sub(quantity)
                            .ok_or(Error::InvalidQuantity)?;
                        (active_bid.bid_purse, new_amount)
                    }
                    None => {
                        // If the target bid does not exist, the function call returns an error
                        return Err(Error::BidNotFound);
                    }
                };

                internal::set_active_bids(self, active_bids)?;
                (bid_purse, new_amount)
            }
        };

        self.unbond(Some(quantity));

        Ok((bonding_purse, new_quantity))
    }

    /// Adds a new delegator to delegators, or tops off a current
    /// one. If the target validator is not in active_bids, the function call
    /// returns a designated “failure” purse and does nothing. The function
    /// calls bond in the Mint contract to transfer motes to the
    /// validator’s purse and returns a tuple of that purse and the
    /// quantity of motes contained in it after the transfer.
    ///
    /// The arguments are the delegator’s key, the originating purse, the validator key and quantity of motes.
    fn delegate(
        &mut self,
        delegator_account_hash: AccountHash,
        source_purse: URef,
        validator_account_hash: AccountHash,
        quantity: U512,
    ) -> Result<(URef, U512)> {
        let active_bids = internal::get_active_bids(self)?;
        // Return early if target validator is not in `active_bids`
        let _active_bid = active_bids
            .get(&validator_account_hash)
            .ok_or(Error::ValidatorNotFound)?;

        let bonding_purse = self.create_purse();
        self.transfer_from_purse_to_purse(source_purse, bonding_purse, quantity)?;
        self.bond(quantity, bonding_purse);

        let new_quantity = {
            let mut delegators = internal::get_delegators(self)?;

            let new_quantity = *delegators
                .entry(delegator_account_hash)
                .or_default()
                .entry(validator_account_hash)
                .and_modify(|delegator| *delegator += quantity)
                .or_insert_with(|| quantity);

            internal::set_delegators(self, delegators)?;

            new_quantity
        };

        Ok((bonding_purse, new_quantity))
    }

    /// Removes a quantity (or the entry altogether, if the
    /// remaining quantity is 0) of motes from the entry in delegators
    /// and calls unbond in the Mint contract to create a new unbonding
    /// purse. Returns the new unbonding purse and the quantity of
    /// remaining delegated motes.
    ///
    /// The arguments are the delegator’s key, the validator key and quantity of motes.
    fn undelegate(
        &mut self,
        delegator_account_hash: AccountHash,
        validator_account_hash: AccountHash,
        quantity: U512,
    ) -> Result<U512> {
        let active_bids = internal::get_active_bids(self)?;
        // Return early if target validator is not in `active_bids`
        let _active_bid = active_bids
            .get(&validator_account_hash)
            .ok_or(Error::ValidatorNotFound)?;

        let mut delegators = internal::get_delegators(self)?;
        let delegators_map = delegators
            .get_mut(&delegator_account_hash)
            .ok_or(Error::DelegatorNotFound)?;
        let amount = delegators_map
            .get_mut(&validator_account_hash)
            .ok_or(Error::ValidatorNotFound)?;

        let new_amount = amount.checked_sub(quantity).ok_or(Error::InvalidQuantity)?;
        if new_amount.is_zero() {
            // Inner map's mapped value should be zero as we subtracted mutable value.
            let _value = delegators_map.remove(&validator_account_hash).unwrap();
            debug_assert!(_value.is_zero());
        }

        internal::set_delegators(self, delegators)?;

        Ok(new_amount)
    }

    /// Removes validator entries from either active_bids or
    /// founding_validators, wherever they might be found. This function
    /// is intended to be called together with the slash function in the
    /// Mint contract.
    /// Access: PoS contractPL
    fn quash_bid(&mut self, validator_account_hashes: &[AccountHash]) -> Result<()> {
        // Clean up inside `founder_validators`
        let mut founding_validators = internal::get_founder_validators(self)?;

        let mut modified_founding_validators = 0usize;

        for validator_account_hash in validator_account_hashes {
            if founding_validators.remove(validator_account_hash).is_some() {
                modified_founding_validators += 1;
            }
        }

        if modified_founding_validators > 0 {
            internal::set_founder_validators(self, founding_validators)?;
        }

        // Clean up inside `active_bids`

        let mut active_bids = internal::get_active_bids(self)?;

        let mut modified_active_bids = 0usize;

        for validator_account_hash in validator_account_hashes {
            if active_bids.remove(validator_account_hash).is_some() {
                modified_active_bids += 1;
            }
        }

        if modified_active_bids > 0 {
            internal::set_active_bids(self, active_bids)?;
        }

        Ok(())
    }

    /// block. Takes active_bids and delegators to construct a list of
    /// validators' total bids (their own added to their delegators') ordered
    /// by size from largest to smallest, then takes the top N (number of
    /// auction slots) bidders and replaced era_validators with these.   
    /// Access: node
    fn run_auction(&mut self) -> Result<()> {
        let active_bids = internal::get_active_bids(self)?;
        let founder_validators = internal::get_founder_validators(self)?;

        // Prepare two iterables containing account hashes with their amounts.
        let active_bids_scores =
            active_bids
                .into_iter()
                .map(|(validator_account_hash, active_bid)| {
                    (validator_account_hash, active_bid.bid_amount)
                });
        let founder_validators_scores = founder_validators
            .into_iter()
            .map(|(validator_account_hash, amount)| (validator_account_hash, amount.staked_amount));

        // Collect validator's entries from both maps into a single vector.
        let mut all_scores: Vec<_> = active_bids_scores
            .chain(founder_validators_scores)
            .collect();

        // Sort by account hashes
        all_scores.sort_by(|(lhs, _), (rhs, _)| lhs.cmp(rhs));

        // All the scores are then grouped by the account hash to calculate a sum of each consecutive scores for each validator.
        // Output of this operation is collected into a binary heap to sort it while
        let mut scores: BinaryHeap<(U512, AccountHash)> = all_scores
            .into_iter()
            .group_by(|(account_hash, _amount)| *account_hash)
            .into_iter()
            .map(|(key, group)| {
                (
                    // Sums consecutive values
                    group.map(|(_, amount)| amount).sum(),
                    key,
                )
            })
            .collect();

        // Take N greatest elements from the heap
        // NOTE: In future we can use into_iter_sorted() which is currently a nightly feature
        let mut era_validators = Vec::with_capacity(AUCTION_SLOTS);
        for _ in 0..AUCTION_SLOTS {
            match scores.pop() {
                Some((_score, account_hash)) => era_validators.push(account_hash),
                None => break,
            }
        }

        internal::set_era_validators(self, era_validators)
    }
}
