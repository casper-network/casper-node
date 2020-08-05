use alloc::{collections::BTreeMap, vec::Vec};

use super::{
    era_validators::ValidatorWeights,
    internal,
    providers::{MintProvider, RuntimeProvider, StorageProvider, SystemProvider},
    seigniorage_recipient::SeigniorageRecipients,
    EraId, EraValidators, SeigniorageRecipient, AUCTION_DELAY, AUCTION_SLOTS,
};
use crate::{
    account::AccountHash,
    auction::{ActiveBid, CommissionRate},
    system_contract_errors::auction::{Error, Result},
    PublicKey, URef, U512,
};

const SYSTEM_ADDR: AccountHash = AccountHash::new([0; 32]);

/// Bonding auctions contract implementation.
pub trait AuctionProvider:
    StorageProvider + SystemProvider + MintProvider + RuntimeProvider
where
    Error: From<<Self as StorageProvider>::Error>
        + From<<Self as SystemProvider>::Error>
        + From<<Self as MintProvider>::Error>,
{
    /// Access: node
    /// Node software will trigger this function once a new era is detected.
    /// The founding_validators data structure is checked, returning False
    /// if the validator is not found and aborting. If the validator is
    /// found, the function first calls release_founder_stake in the Mint
    /// contract. Upon receipt of True, it also flips the relevant field
    /// to True in the validator’s entry within founding_validators.
    /// Otherwise the function aborts.
    fn release_founder(&mut self, public_key: PublicKey) -> Result<bool> {
        if self.get_caller() != SYSTEM_ADDR {
            return Err(Error::InvalidContext);
        }

        // Called by node
        let mut founding_validators = internal::get_founding_validators(self)?;

        // TODO: It should be aware of current time. Era makes more sense.

        match founding_validators.get_mut(&public_key) {
            None => return Ok(false),
            Some(founding_validator) => {
                founding_validator.funds_locked = false;
            }
        }

        internal::set_founding_validators(self, founding_validators)?;

        Ok(true)
    }

    /// Returns era_validators. Publicly accessible, but intended
    /// for periodic use by the PoS contract to update its own internal data
    /// structures recording current and past winners.
    fn read_winners(&mut self) -> Result<EraValidators> {
        internal::get_era_validators(self)
    }

    /// Returns validators in era_validators, mapped to their bids or founding
    /// stakes, delegation rates and lists of delegators together with their
    /// delegated quantities from delegators. This function is publicly
    /// accessible, but intended for system use by the PoS contract, because
    /// this data is necessary for distributing seigniorage.
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

    /// For a non-founder validator, this adds, or modifies, an entry in the
    /// `active_bids` map and calls `bond` in the Mint contract to create (or top off)
    /// a bid purse. It also adjusts the delegation rate.
    /// For a founding validator, the same logic is carried out with
    /// founding_validators, instead of active_bids.
    /// The arguments, in order, are public key, originating purse, delegation
    /// rate and quantity of motes to add. The function returns a tuple of the
    /// bid (or stake) purse key and the new quantity of motes.
    fn add_bid(
        &mut self,
        public_key: PublicKey,
        source_purse: URef,
        delegation_rate: CommissionRate,
        quantity: U512,
    ) -> Result<(URef, U512)> {
        // Creates new purse with desired amount taken from `source_purse`
        // Bonds whole amount from the newly created purse
        let (bonding_purse, _total_amount) = self.bond(public_key, quantity, source_purse)?;

        // Update bids or stakes
        let mut founding_validators = internal::get_founding_validators(self)?;

        let new_quantity = match founding_validators.get_mut(&public_key) {
            // Update `founding_validators` map since `account_hash` belongs to a validator.
            Some(founding_validator) => {
                founding_validator.bonding_purse = bonding_purse;
                founding_validator.delegation_rate = delegation_rate;
                founding_validator.staked_amount += quantity;

                founding_validator.staked_amount
            }
            None => {
                // Non-founder - updates `active_bids` table
                let mut active_bids = internal::get_active_bids(self)?;
                // Returns active bid which could be updated in case given entry exists.
                let bid_amount = {
                    let active_bid = active_bids
                        .entry(public_key)
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
    /// The function returns a tuple of the (new) unbonding purse key and the
    /// new quantity of motes remaining in the bid. If the target bid does not
    /// exist, the function call returns a designated “failure” purse and does nothing.
    ///
    /// The arguments are the public key and amount of motes to remove.
    fn withdraw_bid(&mut self, public_key: PublicKey, quantity: U512) -> Result<(URef, U512)> {
        // Update bids or stakes
        let mut founding_validators = internal::get_founding_validators(self)?;

        let new_quantity = match founding_validators.get_mut(&public_key) {
            Some(founding_validator) if !founding_validator.funds_locked => {
                // Carefully decrease bonded funds

                let new_staked_amount = founding_validator
                    .staked_amount
                    .checked_sub(quantity)
                    .ok_or(Error::InvalidQuantity)?;

                internal::set_founding_validators(self, founding_validators)?;

                new_staked_amount
            }
            Some(_founding_validator) => {
                // If validator is still locked-up (or with an autowin status), no withdrawals
                // are allowed.
                return Err(Error::ValidatorFundsLocked);
            }
            None => {
                let mut active_bids = internal::get_active_bids(self)?;

                // If the target bid does not exist, the function call returns an error
                let active_bid = active_bids.get_mut(&public_key).ok_or(Error::BidNotFound)?;
                let new_amount = active_bid
                    .bid_amount
                    .checked_sub(quantity)
                    .ok_or(Error::InvalidQuantity)?;

                internal::set_active_bids(self, active_bids)?;

                new_amount
            }
        };

        let (unbonding_purse, _total_quantity) = self.unbond(public_key, quantity)?;

        Ok((unbonding_purse, new_quantity))
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
        delegator_public_key: PublicKey,
        source_purse: URef,
        validator_public_key: PublicKey,
        quantity: U512,
    ) -> Result<(URef, U512)> {
        let active_bids = internal::get_active_bids(self)?;
        // Return early if target validator is not in `active_bids`
        let _active_bid = active_bids
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let (bonding_purse, _total_amount) =
            self.bond(delegator_public_key, quantity, source_purse)?;

        let new_quantity = {
            let mut delegators = internal::get_delegations_map(self)?;

            let new_quantity = *delegators
                .entry(validator_public_key)
                .or_default()
                .entry(delegator_public_key)
                .and_modify(|delegator| *delegator += quantity)
                .or_insert_with(|| quantity);

            internal::set_delegations_map(self, delegators)?;

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
    /// The arguments are the delegator's key, the validator key and quantity of motes.
    fn undelegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        quantity: U512,
    ) -> Result<U512> {
        let active_bids = internal::get_active_bids(self)?;

        let (_unbonding_purse, _total_amount) = self.unbond(delegator_public_key, quantity)?;

        // Return early if target validator is not in `active_bids`
        let _active_bid = active_bids
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let mut delegators = internal::get_delegations_map(self)?;
        let delegators_map = delegators
            .get_mut(&validator_public_key)
            .ok_or(Error::DelegatorNotFound)?;

        let new_amount = {
            let amount = delegators_map
                .get_mut(&delegator_public_key)
                .ok_or(Error::ValidatorNotFound)?;

            let new_amount = amount.checked_sub(quantity).ok_or(Error::InvalidQuantity)?;

            *amount = new_amount;
            new_amount
        };

        if new_amount.is_zero() {
            // Inner map's mapped value should be zero as we subtracted mutable value.
            let _value = delegators_map.remove(&validator_public_key).unwrap();
            debug_assert!(_value.is_zero());
        }

        internal::set_delegations_map(self, delegators)?;

        Ok(new_amount)
    }

    /// Removes validator entries from either active_bids or
    /// founding_validators, wherever they might be found. This function
    /// is intended to be called together with the slash function in the
    /// Mint contract.
    /// Access: PoS contractPL
    fn quash_bid(&mut self, validator_public_keys: Vec<PublicKey>) -> Result<()> {
        // Clean up inside `founding_validators`
        let mut founding_validators = internal::get_founding_validators(self)?;

        let mut modified_founding_validators = 0usize;

        for validator_public_key in &validator_public_keys {
            if founding_validators.remove(validator_public_key).is_some() {
                modified_founding_validators += 1;
            }
        }

        if modified_founding_validators > 0 {
            internal::set_founding_validators(self, founding_validators)?;
        }

        // Clean up inside `active_bids`

        let mut active_bids = internal::get_active_bids(self)?;

        let mut modified_active_bids = 0usize;

        for validator_public_key in &validator_public_keys {
            if active_bids.remove(validator_public_key).is_some() {
                modified_active_bids += 1;
            }
        }

        if modified_active_bids > 0 {
            internal::set_active_bids(self, active_bids)?;
        }

        Ok(())
    }

    /// Takes active_bids and delegators to construct a list of validators' total bids
    /// (their own added to their delegators') ordered by size from largest to smallest,
    /// then takes the top N (number of auction slots) bidders and replaced
    /// era_validators with these.
    ///
    /// Accessed by: node
    fn run_auction(&mut self) -> Result<()> {
        if self.get_caller() != SYSTEM_ADDR {
            return Err(Error::InvalidContext);
        }
        let active_bids = internal::get_active_bids(self)?;
        let founding_validators = internal::get_founding_validators(self)?;

        // Take winning validators and add them to validator_weights right away.
        let mut validator_weights: ValidatorWeights = {
            founding_validators
                .iter()
                .filter(|(_validator_account_hash, founding_validator)| {
                    founding_validator.funds_locked
                })
                .map(|(validator_account_hash, amount)| {
                    (*validator_account_hash, amount.staked_amount)
                })
                .collect()
        };

        // Prepare two iterables containing account hashes with their amounts.
        let active_bids_scores = active_bids
            .iter()
            .map(|(validator_account_hash, active_bid)| {
                (*validator_account_hash, active_bid.bid_amount)
            });

        // Non-winning validators are taken care of later
        let founding_validators_scores = founding_validators
            .iter()
            .filter(|(_validator_account_hash, founding_validator)| {
                !founding_validator.funds_locked
            })
            .map(|(validator_account_hash, amount)| {
                (*validator_account_hash, amount.staked_amount)
            });

        // Validator's entries from both maps as a single iterable.
        let all_scores = active_bids_scores.chain(founding_validators_scores);

        // All the scores are then grouped by the account hash to calculate a sum of each consecutive scores for each validator.
        let mut scores = BTreeMap::new();
        for (account_hash, score) in all_scores {
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
        let remaining_auction_slots = AUCTION_SLOTS.saturating_sub(validator_weights.len());
        validator_weights.extend(scores.into_iter().take(remaining_auction_slots));

        let mut era_id = internal::get_era_id(self)?;

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
        for era_validator in validator_weights.keys() {
            let mut seigniorage_recipient = SeigniorageRecipient::default();
            // ... mapped to their bids
            match (
                founding_validators.get(era_validator),
                active_bids.get(era_validator),
            ) {
                (Some(founding_validator), None) => {
                    seigniorage_recipient.stake = founding_validator.staked_amount;
                    seigniorage_recipient.delegation_rate = founding_validator.delegation_rate;
                }
                (None, Some(active_bid)) => {
                    seigniorage_recipient.stake = active_bid.bid_amount;
                    seigniorage_recipient.delegation_rate = active_bid.delegation_rate;
                }
                _ => {
                    // It has to be either of those but can't be in both, or neither of those
                }
            }

            if let Some(delegator_map) = delegators.remove(era_validator) {
                seigniorage_recipient.delegators = delegator_map;
            }

            seigniorage_recipients.insert(*era_validator, seigniorage_recipient);
        }
        let previous_seigniorage_recipients =
            seigniorage_recipients_snapshot.insert(next_era_id, seigniorage_recipients);
        assert!(previous_seigniorage_recipients.is_none());

        internal::set_seigniorage_recipients_snapshot(self, seigniorage_recipients_snapshot)?;

        // Index for next set of validators: `era_id + AUCTION_DELAY`
        let previous_era_validators =
            era_validators.insert(era_id + AUCTION_DELAY, validator_weights);
        assert!(previous_era_validators.is_none());

        internal::set_era_id(self, era_id)?;

        internal::set_era_validators(self, era_validators)
    }

    /// Distributes rewards to the delegators associated with `validator_account_hash`
    fn distribute_to_delegators(&mut self, validator_account_hash: AccountHash, amount: U512) -> Result<()> {
        // let delegations_map= internal::get_delegations(self)?;
        // let delegations = delegations_map.get(&validator_account_hash).unwrap();

        // let tally_map= internal::get_tally(self)?;
        // let tally = tally_map.get(&validator_account_hash).unwrap();

        let total_stake_map= internal::get_total_stake(self)?;
        let total_stake = total_stake_map.get(&validator_account_hash).unwrap();

        if total_stake == 0 {
            todo!(); // throw error
        } 

        let reward_per_stake_map = internal::get_reward_per_stake(self)?;
        let reward_per_stake = reward_per_stake_map.get(&validator_account_hash).unwrap();

        // for (account_hash, rewards) in delegations {

        // }

    }

    /// Reads current era id.
    fn read_era_id(&mut self) -> Result<EraId> {
        internal::get_era_id(self)
    }

}
