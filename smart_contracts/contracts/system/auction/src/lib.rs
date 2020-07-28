#![cfg_attr(not(test), no_std)]

#[macro_use]
extern crate alloc;

mod entry_points;
mod error;
mod providers;

use alloc::vec::Vec;

use casperlabs_types::{
    account::AccountHash,
    auction::{ActiveBid, ActiveBids, DelegationRate, FoundingValidators, SeigniorageRecipients},
    URef, U512,
};

pub use entry_points::get_entry_points;
pub use error::{Error, Result};
use providers::{ProofOfStakeProvider, StorageProvider, SystemProvider};

const FOUNDER_VALIDATORS_KEY: &str = "founder_validators";
const ACTIVE_BIDS_KEY: &str = "active_bids";

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
    fn read_winners(&mut self) -> Vec<AccountHash> {
        todo!()
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
        let founder_validators_key = self
            .get_key(FOUNDER_VALIDATORS_KEY)
            .ok_or(Error::MissingKey)?;
        let founder_validators_uref = founder_validators_key
            .into_uref()
            .ok_or(Error::InvalidKeyVariant)?;

        let mut founder_validators: FoundingValidators = self
            .read(founder_validators_uref)?
            .ok_or(Error::MissingValue)?;
        let new_quantity = match founder_validators.get_mut(&account_hash) {
            // Update `founder_validators` map since `account_hash` belongs to a validator.
            Some(founding_validator) => {
                founding_validator.bonding_purse = bonding_purse;
                founding_validator.staked_amount += quantity;

                founding_validator.staked_amount
            }
            None => {
                // Non-founder - updates `active_bids` table
                let active_bids_key = self.get_key(ACTIVE_BIDS_KEY).ok_or(Error::MissingKey)?;
                let active_bids_uref = active_bids_key
                    .into_uref()
                    .ok_or(Error::InvalidKeyVariant)?;
                let mut active_bids: ActiveBids =
                    self.read(active_bids_uref)?.ok_or(Error::MissingValue)?;
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
                self.write(active_bids_uref, active_bids);

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
    fn withdraw_bid(
        &mut self,
        _account_hash: AccountHash,
        _quantity: U512,
    ) -> Result<(URef, U512)> {
        todo!()
    }

    /// Adds a new delegator to delegators, or tops off a curren
    /// one. If the target validator is not in active_bids, the function call
    /// returns a designated “failure” purse and does nothing. The function
    /// calls bond in the Mint contract to transfer motes to the
    /// validator’s purse and returns a tuple of that purse and the
    /// quantity of motes contained in it after the transfer.
    ///
    /// The arguments are the delegator’s key, the originating purse, the validator key and quantity of motes.
    fn delegate(
        &mut self,
        _delegator_account_hash: AccountHash,
        _source_purse: URef,
        _validator_account_hash: AccountHash,
        _quantity: U512,
    ) -> Result<(URef, U512)> {
        todo!()
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
        _delegator_account_hash: AccountHash,
        _validator_account_hash: AccountHash,
        _quantity: U512,
    ) -> Result<(URef, U512)> {
        todo!()
    }

    /// Removes validator entries from either active_bids or
    /// founding_validators, wherever they might be found. This function
    /// is intended to be called together with the slash function in the
    /// Mint contract.
    /// Access: PoS contractPL
    fn quash_bid(&mut self, _validator_keys: &[AccountHash]) {
        todo!()
    }

    /// block. Takes active_bids and delegators to construct a list of
    /// validators' total bids (their own added to their delegators') ordered
    /// by size from largest to smallest, then takes the top N (number of
    /// auction slots) bidders and replaced era_validators with these.   
    /// Access: node
    fn run_auction(&mut self) {
        todo!()
    }
}
