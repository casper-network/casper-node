use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::{
    era_validators::ValidatorWeights,
    internal,
    providers::{RuntimeProvider, StorageProvider, SystemProvider},
    seigniorage_recipient::SeigniorageRecipients,
    EraId, EraValidators, SeigniorageRecipient, AUCTION_DELAY, AUCTION_SLOTS, SNAPSHOT_SIZE,
};
use crate::{
    account::AccountHash,
    auction::{
        unbonding_purse::{UnbondingPurse, UnbondingPurses},
        ActiveBid, DelegationRate,
    },
    system_contract_errors::auction::{Error, Result},
    Key, PublicKey, URef, U512,
};

/// Bidders mapped to their bidding purses and tokens contained therein. Delegators' tokens
/// are kept in the validator bid purses, available for withdrawal up to the delegated number
/// of tokens. Withdrawal moves the tokens to a delegator-controlled unbonding purse.
pub type BidPurses = BTreeMap<PublicKey, URef>;
//
// /// Founding validators mapped to their staking purses and tokens contained therein. These
// /// function much like the regular bidding purses, but have a field indicating whether any tokens
// /// may be unbonded.
// pub type FounderPurses = BTreeMap<AccountHash, (URef, bool)>;

/// Name of bid purses named key.
pub const BID_PURSES_KEY: &str = "bid_purses";
// /// Name of founder purses named key.
// pub const FOUNDER_PURSES_KEY: &str = "founder_purses";
/// Name of unbonding purses key.
pub const UNBONDING_PURSES_KEY: &str = "unbonding_purses";

/// Default number of eras that need to pass to be able to withdraw unbonded funds.
pub const DEFAULT_UNBONDING_DELAY: u64 = 14;

const SYSTEM_ACCOUNT: AccountHash = AccountHash::new([0; 32]);

/// Bonding auctions contract implementation.
pub trait AuctionProvider: StorageProvider + SystemProvider + RuntimeProvider
where
    Error: From<<Self as StorageProvider>::Error> + From<<Self as SystemProvider>::Error>,
{
    /// The `founding_validators` data structure is checked, returning False if the validator is
    /// not found and returns a `false` early. If the validator is found, then validator's entry is
    /// updated so the funds are unlocked and a `true` is returned.
    ///
    /// For security reasons this entry point can be only called by node.
    fn release_founder(&mut self, public_key: PublicKey) -> Result<bool> {
        if self.get_caller() != SYSTEM_ACCOUNT {
            return Err(Error::InvalidContext);
        }

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

    /// For a non-founder validator, this adds, or modifies, an entry in the `active_bids` map and
    /// calls `bond` in the Mint contract to create (or top off) a bid purse. It also adjusts the
    /// delegation rate.
    ///
    /// For a founding validator, the same logic is carried out with founding_validators, instead
    /// of `active_bids`.
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
        let mut founding_validators = internal::get_founding_validators(self)?;

        let new_amount = match founding_validators.get_mut(&public_key) {
            // Update `founding_validators` map since `account_hash` belongs to a validator.
            Some(founding_validator) => {
                founding_validator.bonding_purse = bonding_purse;
                founding_validator.delegation_rate = delegation_rate;
                founding_validator.staked_amount += amount;

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
                            active_bid.bid_amount += amount;
                            active_bid.bid_purse = bonding_purse;
                            active_bid.delegation_rate = delegation_rate;
                        })
                        .or_insert_with(|| {
                            // Create new entry in active bids
                            ActiveBid {
                                bid_purse: bonding_purse,
                                bid_amount: amount,
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

        Ok((bonding_purse, new_amount))
    }

    /// For a non-founder validator, implements essentially the same logic as add_bid, but reducing
    /// the number of tokens and calling unbond in lieu of bond.
    ///
    /// For a founding validator, this function first checks whether they are released, and fails
    /// if they are not. Additionally, the relevant data structure is founding_validators, rather
    /// than active_bids.
    ///
    /// The function returns a tuple of the (new) unbonding purse key and the new quantity of motes
    /// remaining in the bid. If the target bid does not exist, the function call returns an error.
    fn withdraw_bid(&mut self, public_key: PublicKey, amount: U512) -> Result<(URef, U512)> {
        // Update bids or stakes
        let mut founding_validators = internal::get_founding_validators(self)?;

        let new_quantity = match founding_validators.get_mut(&public_key) {
            Some(founding_validator) if !founding_validator.funds_locked => {
                // Carefully decrease bonded funds

                let new_staked_amount = founding_validator
                    .staked_amount
                    .checked_sub(amount)
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
                    .checked_sub(amount)
                    .ok_or(Error::InvalidQuantity)?;

                internal::set_active_bids(self, active_bids)?;

                new_amount
            }
        };

        let (unbonding_purse, _total_quantity) = self.unbond(public_key, amount)?;

        Ok((unbonding_purse, new_quantity))
    }

    /// Adds a new delegator to delegators, or tops off a current one. If the target validator is
    /// not in active_bids, the function call returns an error and does nothing.
    ///
    /// The function calls bond in the Mint contract to transfer motes to the validator's purse and
    /// returns a tuple of that purse and the quantity of motes contained in it after the transfer.
    fn delegate(
        &mut self,
        delegator_public_key: PublicKey,
        source: URef,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Result<(URef, U512)> {
        let active_bids = internal::get_active_bids(self)?;
        // Return early if target validator is not in `active_bids`
        let _active_bid = active_bids
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let (bonding_purse, _total_amount) = self.bond(delegator_public_key, source, amount)?;

        let new_quantity = {
            let mut delegators = internal::get_delegators(self)?;

            let new_quantity = *delegators
                .entry(validator_public_key)
                .or_default()
                .entry(delegator_public_key)
                .and_modify(|delegator| *delegator += amount)
                .or_insert_with(|| amount);

            internal::set_delegators(self, delegators)?;

            new_quantity
        };

        Ok((bonding_purse, new_quantity))
    }

    /// Removes a quantity (or the entry altogether, if the remaining quantity is 0) of motes from
    /// the entry in delegators and calls unbond in the Mint contract to create a new unbonding
    /// purse.
    ///
    /// Returns the new unbonding purse and the quantity of remaining delegated motes.
    fn undelegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Result<U512> {
        let active_bids = internal::get_active_bids(self)?;

        let (_unbonding_purse, _total_amount) = self.unbond(delegator_public_key, amount)?;

        // Return early if target validator is not in `active_bids`
        let _active_bid = active_bids
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let mut delegators = internal::get_delegators(self)?;
        let delegators_map = delegators
            .get_mut(&validator_public_key)
            .ok_or(Error::DelegatorNotFound)?;

        let new_amount = {
            let delegators_amount = delegators_map
                .get_mut(&delegator_public_key)
                .ok_or(Error::ValidatorNotFound)?;

            let new_amount = delegators_amount
                .checked_sub(amount)
                .ok_or(Error::InvalidQuantity)?;

            *delegators_amount = new_amount;
            new_amount
        };

        if new_amount.is_zero() {
            // Inner map's mapped value should be zero as we subtracted mutable value.
            let _value = delegators_map.remove(&validator_public_key).unwrap();
            debug_assert!(_value.is_zero());
        }

        internal::set_delegators(self, delegators)?;

        Ok(new_amount)
    }

    /// Removes validator entries from either active_bids or founding_validators, wherever they
    /// might be found.
    ///
    /// This function is intended to be called together with the slash function in the Mint
    /// contract.
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

    /// Creates a new purse in bid_purses corresponding to a validator's key, or tops off an
    /// existing one.
    ///
    /// Returns the bid purse's key and current quantity of motes.
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

    /// Creates a new purse in unbonding_purses given a validator's key and quantity, returning
    /// the new purse's key and the quantity of motes remaining in the validator's bid purse.
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

    /// Iterates over unbonding entries and checks if a locked amount can be paid already if
    /// a specific era is reached.
    ///
    /// This entry point can be called by a system only.
    fn process_unbond_requests(&mut self) -> Result<()> {
        if self.get_caller() != SYSTEM_ACCOUNT {
            return Err(Error::InvalidCaller);
        }
        let bid_purses_uref = self
            .get_key(BID_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;

        let bid_purses: BidPurses = self.read(bid_purses_uref)?.ok_or(Error::Storage)?;

        // Update `unbonding_purses` data
        let unbonding_purses_uref = self
            .get_key(UNBONDING_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;
        let mut unbonding_purses: UnbondingPurses =
            self.read(unbonding_purses_uref)?.ok_or(Error::Storage)?;

        let current_era_id = self.read_era_id()?;

        for unbonding_list in unbonding_purses.values_mut() {
            let mut new_unbonding_list = Vec::new();
            for unbonding_purse in unbonding_list.iter() {
                let source = bid_purses
                    .get(&unbonding_purse.origin)
                    .ok_or(Error::BondNotFound)?;
                // Since `process_unbond_requests` is run before `run_auction`, so we should check
                // if current era id is equal or greater than the `era_of_withdrawal` that was
                // calculated on `unbond` attempt.
                if current_era_id >= unbonding_purse.era_of_withdrawal as u64 {
                    // Move funds from bid purse to unbonding purse
                    self.transfer_from_purse_to_purse(
                        *source,
                        unbonding_purse.purse,
                        unbonding_purse.amount,
                    )?;
                } else {
                    new_unbonding_list.push(*unbonding_purse);
                }
            }
            *unbonding_list = new_unbonding_list;
        }
        self.write(unbonding_purses_uref, unbonding_purses)?;
        Ok(())
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

        let seigniorage_recipients_snapshot = seigniorage_recipients_snapshot
            .into_iter()
            .rev()
            .take(SNAPSHOT_SIZE)
            .collect();
        internal::set_seigniorage_recipients_snapshot(self, seigniorage_recipients_snapshot)?;

        // Index for next set of validators: `era_id + AUCTION_DELAY`
        let previous_era_validators =
            era_validators.insert(era_id + AUCTION_DELAY, validator_weights);
        assert!(previous_era_validators.is_none());

        internal::set_era_id(self, era_id)?;

        // Keep maximum of `AUCTION_DELAY + 1` elements
        let era_validators = era_validators
            .into_iter()
            .rev()
            .take(SNAPSHOT_SIZE)
            .collect();

        internal::set_era_validators(self, era_validators)
    }

    /// Reads current era id.
    fn read_era_id(&mut self) -> Result<EraId> {
        internal::get_era_id(self)
    }
}
