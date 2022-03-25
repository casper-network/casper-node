pub(crate) mod detail;
pub(crate) mod providers;

use std::collections::BTreeMap;

use num_rational::Ratio;
use num_traits::{CheckedMul, CheckedSub};

use casper_types::{
    account::AccountHash,
    system::auction::{
        Bid, DelegationRate, EraInfo, EraValidators, Error, SeigniorageRecipients,
        ValidatorWeights, BLOCK_REWARD, DELEGATION_RATE_DENOMINATOR,
    },
    ApiError, EraId, PublicKey, U512,
};

use self::providers::{AccountProvider, MintProvider, RuntimeProvider, StorageProvider};

/// Bonding auction contract interface
pub trait Auction:
    StorageProvider + RuntimeProvider + MintProvider + AccountProvider + Sized
{
    /// Returns active validators and auction winners for a number of future eras determined by the
    /// configured auction_delay.
    fn get_era_validators(&mut self) -> Result<EraValidators, Error> {
        let snapshot = detail::get_seigniorage_recipients_snapshot(self)?;
        let era_validators = detail::era_validators_from_snapshot(snapshot);
        Ok(era_validators)
    }

    /// Returns validators in era_validators, mapped to their bids or founding stakes, delegation
    /// rates and lists of delegators together with their delegated quantities from delegators.
    /// This function is publicly accessible, but intended for system use by the Handle Payment
    /// contract, because this data is necessary for distributing seigniorage.
    fn read_seigniorage_recipients(&mut self) -> Result<SeigniorageRecipients, Error> {
        // `era_validators` are assumed to be computed already by calling "run_auction" entrypoint.
        let era_index = detail::get_era_id(self)?;
        let mut seigniorage_recipients_snapshot =
            detail::get_seigniorage_recipients_snapshot(self)?;
        let seigniorage_recipients = seigniorage_recipients_snapshot
            .remove(&era_index)
            .ok_or(Error::MissingSeigniorageRecipients)?;
        Ok(seigniorage_recipients)
    }

    /// This entry point adds or modifies an entry in the `Key::Bid` section of the global state and
    /// creates (or tops off) a bid purse. Post genesis, any new call on this entry point causes a
    /// non-founding validator in the system to exist.
    ///
    /// The logic works for both founding and non-founding validators, making it possible to adjust
    /// their delegation rate and increase their stakes.
    ///
    /// A validator with its bid inactive due to slashing can activate its bid again by increasing
    /// its stake.
    ///
    /// Validators cannot create a bid with 0 amount, and the delegation rate can't exceed
    /// [`DELEGATION_RATE_DENOMINATOR`].
    ///
    /// Returns a [`U512`] value indicating total amount of tokens staked for given `public_key`.
    fn add_bid(
        &mut self,
        public_key: PublicKey,
        delegation_rate: DelegationRate,
        amount: U512,
    ) -> Result<U512, ApiError> {
        let provided_account_hash = AccountHash::from_public_key(&public_key, |x| self.blake2b(x));

        if amount.is_zero() {
            return Err(Error::BondTooSmall.into());
        }

        if delegation_rate > DELEGATION_RATE_DENOMINATOR {
            return Err(Error::DelegationRateTooLarge.into());
        }

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext.into());
        }

        let source = self.get_main_purse()?;

        let account_hash = AccountHash::from(&public_key);

        // Update bids or stakes
        let updated_amount = match self.read_bid(&account_hash)? {
            Some(mut bid) => {
                if bid.inactive() {
                    bid.activate();
                }
                self.mint_transfer_direct(
                    Some(PublicKey::System.to_account_hash()),
                    source,
                    *bid.bonding_purse(),
                    amount,
                    None,
                )
                .map_err(|_| ApiError::from(Error::TransferToBidPurse))?
                .map_err(|mint_error| {
                    // Propagate mint contract's error that occured during execution of transfer
                    // entrypoint. This will improve UX in case of (for example)
                    // unapproved spending limit error.
                    ApiError::from(mint_error)
                })?;
                let updated_amount = bid
                    .with_delegation_rate(delegation_rate)
                    .increase_stake(amount)?;
                self.write_bid(account_hash, bid)?;
                updated_amount
            }
            None => {
                let bonding_purse = self.create_purse()?;
                self.mint_transfer_direct(
                    Some(PublicKey::System.to_account_hash()),
                    source,
                    bonding_purse,
                    amount,
                    None,
                )
                .map_err(|_| Error::TransferToBidPurse)?
                .map_err(|mint_error| {
                    // Propagate mint contract's error that occured during execution of transfer
                    // entrypoint. This will improve UX in case of (for example)
                    // unapproved spending limit error.
                    ApiError::from(mint_error)
                })?;
                let bid = Bid::unlocked(public_key, bonding_purse, amount, delegation_rate);
                self.write_bid(account_hash, bid)?;
                amount
            }
        };

        Ok(updated_amount)
    }

    /// For a non-founder validator, this method simply decreases a stake.
    ///
    /// For a founding validator, this function first checks whether they are released and fails if
    /// they are not.
    ///
    /// When a validator's stake reaches 0 all the delegators that bid their tokens to it are
    /// automatically unbonded with their total staked amount and the bid is deactivated. A bid can
    /// be activated again by staking tokens.
    ///
    /// You can't withdraw higher amount than its currently staked. Withdrawing zero is allowed,
    /// although it does not change the state of the auction.
    ///
    /// The function returns the new amount of motes remaining in the bid. If the target bid does
    /// not exist, the function call returns an error.
    fn withdraw_bid(&mut self, public_key: PublicKey, amount: U512) -> Result<U512, Error> {
        let provided_account_hash = AccountHash::from_public_key(&public_key, |x| self.blake2b(x));

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext);
        }

        let mut bid = self
            .read_bid(&provided_account_hash)?
            .ok_or(Error::ValidatorNotFound)?;

        let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;

        // Fails if requested amount is greater than either the total stake or the amount of vested
        // stake.
        let updated_stake = bid.decrease_stake(amount, era_end_timestamp_millis)?;

        detail::create_unbonding_purse(
            self,
            public_key.clone(),
            public_key.clone(), // validator is the unbonder
            *bid.bonding_purse(),
            amount,
            None,
        )?;

        if updated_stake.is_zero() {
            // Automatically unbond delegators
            for (delegator_public_key, delegator) in bid.delegators() {
                detail::create_unbonding_purse(
                    self,
                    public_key.clone(),
                    delegator_public_key.clone(),
                    *delegator.bonding_purse(),
                    *delegator.staked_amount(),
                    None,
                )?;
            }

            *bid.delegators_mut() = BTreeMap::new();

            bid.deactivate();
        }

        self.write_bid(provided_account_hash, bid)?;

        Ok(updated_stake)
    }

    /// Adds a new delegator to delegators or increases its current stake. If the target validator
    /// is missing, the function call returns an error and does nothing.
    ///
    /// The function transfers motes from the source purse to the delegator's bonding purse.
    ///
    /// This entry point returns the number of tokens currently delegated to a given validator.
    fn delegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
        minimum_delegation_amount: u64,
    ) -> Result<U512, ApiError> {
        let provided_account_hash =
            AccountHash::from_public_key(&delegator_public_key, |x| self.blake2b(x));

        if amount.is_zero() {
            return Err(Error::BondTooSmall.into());
        }

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext.into());
        }

        let source = self.get_main_purse()?;

        let validator_account_hash = AccountHash::from(&validator_public_key);

        let bid = detail::read_bid_for_validator(self, validator_account_hash)?;

        if amount < U512::from(minimum_delegation_amount) {
            return Err(Error::DelegationAmountTooSmall.into());
        }

        detail::handle_delegation(
            self,
            bid,
            delegator_public_key,
            validator_public_key,
            source,
            amount,
        )
    }

    /// Removes specified amount of motes (or the value from the collection altogether, if the
    /// remaining amount is 0) from the entry in delegators map for given validator and creates a
    /// new unbonding request to the queue.
    ///
    /// The arguments are the delegator's key, the validator's key, and the amount.
    ///
    /// Returns the remaining bid amount after the stake was decreased.
    fn undelegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Result<U512, Error> {
        let provided_account_hash =
            AccountHash::from_public_key(&delegator_public_key, |x| self.blake2b(x));

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext);
        }

        let validator_account_hash = AccountHash::from(&validator_public_key);

        let mut bid = match self.read_bid(&validator_account_hash)? {
            Some(bid) => bid,
            None => return Err(Error::ValidatorNotFound),
        };

        let delegators = bid.delegators_mut();

        let new_amount = match delegators.get_mut(&delegator_public_key) {
            Some(delegator) => {
                detail::create_unbonding_purse(
                    self,
                    validator_public_key,
                    delegator_public_key.clone(),
                    *delegator.bonding_purse(),
                    amount,
                    None,
                )?;

                let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;
                let updated_stake = delegator.decrease_stake(amount, era_end_timestamp_millis)?;
                if updated_stake == U512::zero() {
                    delegators.remove(&delegator_public_key);
                };
                updated_stake
            }
            None => return Err(Error::DelegatorNotFound),
        };

        self.write_bid(validator_account_hash, bid)?;

        Ok(new_amount)
    }

    /// Removes specified amount of motes (or the value from the collection altogether, if the
    /// remaining amount is 0) from the entry in delegators map for given validator and creates a
    /// new unbonding request to the queue, which when processed will redelegate the
    /// specified amount of motes to new validator.
    ///
    /// The arguments are the delegator's key, the validator's key, the amount,
    /// and the new validator's key.
    ///
    /// Returns the remaining bid amount if the new validator is inactive.
    fn redelegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
        new_validator: PublicKey,
        minimum_delegation_amount: u64,
    ) -> Result<U512, Error> {
        let provided_account_hash =
            AccountHash::from_public_key(&delegator_public_key, |x| self.blake2b(x));

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext);
        }

        if amount < U512::from(minimum_delegation_amount) {
            return Err(Error::DelegationAmountTooSmall);
        }

        let validator_account_hash = AccountHash::from(&validator_public_key);

        let mut bid = match self.read_bid(&validator_account_hash)? {
            Some(bid) => bid,
            None => return Err(Error::ValidatorNotFound),
        };

        let delegators = bid.delegators_mut();

        let new_amount = match delegators.get_mut(&delegator_public_key) {
            Some(delegator) => {
                detail::create_unbonding_purse(
                    self,
                    validator_public_key,
                    delegator_public_key.clone(),
                    *delegator.bonding_purse(),
                    amount,
                    Some(new_validator),
                )?;

                let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;
                let updated_stake = delegator.decrease_stake(amount, era_end_timestamp_millis)?;
                if updated_stake == U512::zero() {
                    delegators.remove(&delegator_public_key);
                };
                updated_stake
            }
            None => return Err(Error::DelegatorNotFound),
        };

        self.write_bid(validator_account_hash, bid)?;

        Ok(new_amount)
    }

    /// Slashes each validator.
    ///
    /// This can be only invoked through a system call.
    fn slash(&mut self, validator_public_keys: Vec<PublicKey>) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller);
        }

        let mut burned_amount: U512 = U512::zero();

        for validator_public_key in validator_public_keys {
            // Burn stake, deactivate
            let validator_account_hash = AccountHash::from(&validator_public_key);
            if let Some(mut bid) = self.read_bid(&validator_account_hash)? {
                burned_amount += *bid.staked_amount();
                *bid.staked_amount_mut() = U512::zero();
                bid.deactivate();
                // Reset delegator stakes when deactivating validator bid.
                for delegator in bid.delegators_mut().values_mut() {
                    *delegator.staked_amount_mut() = U512::zero();
                }
                self.write_bid(validator_account_hash, bid)?;
            };

            let validator_account_hash = AccountHash::from(&validator_public_key);
            // Update unbonding entries for given validator
            let unbonding_purses = self.read_unbond(&validator_account_hash)?;
            if !unbonding_purses.is_empty() {
                burned_amount += unbonding_purses
                    .into_iter()
                    .map(|unbonding_purse| *unbonding_purse.amount())
                    .sum();
                self.write_unbond(validator_account_hash, Vec::new())?;
            }
        }

        self.reduce_total_supply(burned_amount)?;

        Ok(())
    }

    /// Takes active_bids and delegators to construct a list of validators' total bids (their own
    /// added to their delegators') ordered by size from largest to smallest, then takes the top N
    /// (number of auction slots) bidders and replaces era_validators with these.
    ///
    /// Accessed by: node
    fn run_auction(
        &mut self,
        era_end_timestamp_millis: u64,
        evicted_validators: Vec<PublicKey>,
    ) -> Result<(), ApiError> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller.into());
        }

        let validator_slots = detail::get_validator_slots(self)?;
        let auction_delay = detail::get_auction_delay(self)?;
        let snapshot_size = auction_delay as usize + 1;
        let mut era_id: EraId = detail::get_era_id(self)?;
        let mut bids = detail::get_bids(self)?;

        // Process unbond requests
        detail::process_unbond_requests(self)?;

        // Process bids
        let mut bids_modified = false;
        for (validator_public_key, bid) in bids.iter_mut() {
            if bid.process(era_end_timestamp_millis) {
                bids_modified = true;
            }

            if evicted_validators.contains(validator_public_key) {
                bids_modified = bid.deactivate()
            }
        }

        // Compute next auction winners
        let winners: ValidatorWeights = {
            let locked_validators: ValidatorWeights = bids
                .iter()
                .filter(|(_public_key, bid)| {
                    bid.is_locked(era_end_timestamp_millis) && !bid.inactive()
                })
                .map(|(public_key, bid)| {
                    let total_staked_amount = bid.total_staked_amount()?;
                    Ok((public_key.clone(), total_staked_amount))
                })
                .collect::<Result<ValidatorWeights, Error>>()?;

            // We collect these into a vec for sorting
            let mut unlocked_validators: Vec<(PublicKey, U512)> = bids
                .iter()
                .filter(|(_public_key, bid)| {
                    !bid.is_locked(era_end_timestamp_millis) && !bid.inactive()
                })
                .map(|(public_key, bid)| {
                    let total_staked_amount = bid.total_staked_amount()?;
                    Ok((public_key.clone(), total_staked_amount))
                })
                .collect::<Result<Vec<(PublicKey, U512)>, Error>>()?;

            unlocked_validators.sort_by(|(_, lhs), (_, rhs)| rhs.cmp(lhs));

            // This assumes that amount of founding validators does not exceed configured validator
            // slots. For a case where there are exactly N validators and the limit is N, only
            // founding validators will be the in the winning set. It is advised to set
            // `validator_slots` larger than amount of founding validators in accounts.toml to
            // accomodate non-genesis validators.
            let remaining_auction_slots = validator_slots.saturating_sub(locked_validators.len());

            locked_validators
                .into_iter()
                .chain(
                    unlocked_validators
                        .into_iter()
                        .take(remaining_auction_slots),
                )
                .collect()
        };

        // Increment era
        era_id = era_id.checked_add(1).ok_or(Error::ArithmeticOverflow)?;

        let delayed_era = era_id
            .checked_add(auction_delay)
            .ok_or(Error::ArithmeticOverflow)?;

        // Update seigniorage recipients for current era
        {
            let mut snapshot = detail::get_seigniorage_recipients_snapshot(self)?;

            let mut recipients = SeigniorageRecipients::new();

            for era_validator in winners.keys() {
                let seigniorage_recipient = match bids.get(era_validator) {
                    Some(bid) => bid.into(),
                    None => return Err(Error::BidNotFound.into()),
                };
                recipients.insert(era_validator.clone(), seigniorage_recipient);
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
    fn distribute(&mut self, reward_factors: BTreeMap<PublicKey, u64>) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller);
        }

        let seigniorage_recipients = self.read_seigniorage_recipients()?;
        let base_round_reward = self.read_base_round_reward()?;
        let era_id = detail::get_era_id(self)?;

        let mut era_info = EraInfo::new();
        let seigniorage_allocations = era_info.seigniorage_allocations_mut();

        for (public_key, reward_factor) in reward_factors {
            let recipient = seigniorage_recipients
                .get(&public_key)
                .ok_or(Error::ValidatorNotFound)?;

            let total_stake = recipient.total_stake().ok_or(Error::ArithmeticOverflow)?;
            if total_stake.is_zero() {
                // TODO: error?
                continue;
            }

            let total_reward: Ratio<U512> = {
                let reward_rate = Ratio::new(U512::from(reward_factor), U512::from(BLOCK_REWARD));
                reward_rate
                    .checked_mul(&Ratio::from(base_round_reward))
                    .ok_or(Error::ArithmeticOverflow)?
            };

            let delegator_total_stake: U512 = recipient
                .delegator_total_stake()
                .ok_or(Error::ArithmeticOverflow)?;

            let delegators_part: Ratio<U512> = {
                let commission_rate = Ratio::new(
                    U512::from(*recipient.delegation_rate()),
                    U512::from(DELEGATION_RATE_DENOMINATOR),
                );
                let reward_multiplier: Ratio<U512> = Ratio::new(delegator_total_stake, total_stake);
                let delegator_reward: Ratio<U512> = total_reward
                    .checked_mul(&reward_multiplier)
                    .ok_or(Error::ArithmeticOverflow)?;
                let commission: Ratio<U512> = delegator_reward
                    .checked_mul(&commission_rate)
                    .ok_or(Error::ArithmeticOverflow)?;
                delegator_reward
                    .checked_sub(&commission)
                    .ok_or(Error::ArithmeticOverflow)?
            };

            let delegator_rewards =
                recipient
                    .delegator_stake()
                    .iter()
                    .map(|(delegator_key, delegator_stake)| {
                        let reward_multiplier = Ratio::new(*delegator_stake, delegator_total_stake);
                        let reward = delegators_part * reward_multiplier;
                        (delegator_key.clone(), reward)
                    });
            let delegator_payouts = detail::reinvest_delegator_rewards(
                self,
                seigniorage_allocations,
                public_key.clone(),
                delegator_rewards,
            )?;
            let total_delegator_payout: U512 = delegator_payouts
                .iter()
                .map(|(_delegator_hash, amount, _bonding_purse)| *amount)
                .sum();

            let validators_part: Ratio<U512> = total_reward - Ratio::from(total_delegator_payout);
            let validator_reward = validators_part.to_integer();
            let validator_bonding_purse = detail::reinvest_validator_reward(
                self,
                seigniorage_allocations,
                public_key.clone(),
                validator_reward,
            )?;

            self.mint_into_existing_purse(validator_reward, validator_bonding_purse)
                .map_err(Error::from)?;

            for (_delegator_account_hash, delegator_payout, bonding_purse) in delegator_payouts {
                self.mint_into_existing_purse(delegator_payout, bonding_purse)
                    .map_err(Error::from)?;
            }
        }

        self.record_era_info(era_id, era_info)?;

        Ok(())
    }

    /// Reads current era id.
    fn read_era_id(&mut self) -> Result<EraId, Error> {
        detail::get_era_id(self)
    }

    /// Activates a given validator's bid.  To be used when a validator has been marked as inactive
    /// by consensus (aka "evicted").
    fn activate_bid(&mut self, validator_public_key: PublicKey) -> Result<(), Error> {
        let provided_account_hash =
            AccountHash::from_public_key(&validator_public_key, |x| self.blake2b(x));

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext);
        }

        let mut bid = match self.read_bid(&provided_account_hash)? {
            Some(bid) => bid,
            None => return Err(Error::ValidatorNotFound),
        };

        bid.activate();

        self.write_bid(provided_account_hash, bid)?;

        Ok(())
    }
}
