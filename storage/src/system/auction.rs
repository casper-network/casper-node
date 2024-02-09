pub mod bidding;
pub mod detail;
pub mod providers;

use std::collections::BTreeMap;

use num_rational::Ratio;
use num_traits::{CheckedMul, CheckedSub};
use tracing::debug;

use crate::system::auction::detail::{
    process_with_vesting_schedule, read_delegator_bid, read_delegator_bids, read_validator_bid,
    seigniorage_recipient,
};
use casper_types::{
    account::AccountHash,
    system::auction::{
        BidAddr, BidKind, DelegationRate, EraInfo, EraValidators, Error, SeigniorageRecipients,
        UnbondingPurse, ValidatorBid, ValidatorWeights, DELEGATION_RATE_DENOMINATOR,
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
        if !self.allow_auction_bids() {
            // Validation set rotation might be disabled on some private chains and we should not
            // allow new bids to come in.
            return Err(Error::AuctionBidsDisabled.into());
        }

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

        let validator_bid_key = BidAddr::from(public_key.clone()).into();
        let (target, validator_bid) = if let Some(BidKind::Validator(mut validator_bid)) =
            self.read_bid(&validator_bid_key)?
        {
            validator_bid.increase_stake(amount)?;
            validator_bid.with_delegation_rate(delegation_rate);
            (*validator_bid.bonding_purse(), validator_bid)
        } else {
            let bonding_purse = self.create_purse()?;
            let validator_bid =
                ValidatorBid::unlocked(public_key, bonding_purse, amount, delegation_rate);
            (bonding_purse, Box::new(validator_bid))
        };

        let source = self.get_main_purse()?;
        self.mint_transfer_direct(
            Some(PublicKey::System.to_account_hash()),
            source,
            target,
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

        let updated_amount = validator_bid.staked_amount();
        self.write_bid(validator_bid_key, BidKind::Validator(validator_bid))?;
        Ok(updated_amount)
    }

    /// Unbonds aka reduces stake by specified amount, adding an entry to the unbonding queue.
    /// For a genesis validator, this is subject to vesting if applicable to a given network.
    ///
    /// If this bid stake is reduced to 0, any delegators to this bid will be undelegated, with
    /// entries made to the unbonding queue for each of them for their full delegated amount.
    /// Additionally, this bid record will be pruned away from the next calculated root hash.
    ///
    /// An attempt to reduce stake by more than is staked will instead 0 the stake.
    ///
    /// The function returns the remaining staked amount (we allow partial unbonding).
    fn withdraw_bid(&mut self, public_key: PublicKey, amount: U512) -> Result<U512, Error> {
        let provided_account_hash = AccountHash::from_public_key(&public_key, |x| self.blake2b(x));

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext);
        }

        let validator_bid_addr = BidAddr::from(public_key.clone());
        let validator_bid_key = validator_bid_addr.into();
        let mut validator_bid = read_validator_bid(self, &validator_bid_key)?;
        let initial_amount = validator_bid.staked_amount();

        // An attempt to unbond more than is staked results in unbonding the staked amount.
        let unbonding_amount = U512::min(amount, validator_bid.staked_amount());

        let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;
        let updated_stake =
            validator_bid.decrease_stake(unbonding_amount, era_end_timestamp_millis)?;

        detail::create_unbonding_purse(
            self,
            public_key.clone(),
            public_key.clone(), // validator is the unbonder
            *validator_bid.bonding_purse(),
            unbonding_amount,
            None,
        )?;

        debug!(
            "withdrawing bid for {} reducing {} by {} to {}",
            validator_bid_addr, initial_amount, unbonding_amount, updated_stake
        );
        if updated_stake.is_zero() {
            // Unbond all delegators and zero them out
            let delegators = read_delegator_bids(self, &public_key)?;
            for mut delegator in delegators {
                let delegator_public_key = delegator.delegator_public_key().clone();
                detail::create_unbonding_purse(
                    self,
                    public_key.clone(),
                    delegator_public_key.clone(),
                    *delegator.bonding_purse(),
                    delegator.staked_amount(),
                    None,
                )?;
                delegator.decrease_stake(delegator.staked_amount(), era_end_timestamp_millis)?;
                let delegator_bid_addr =
                    BidAddr::new_from_public_keys(&public_key, Some(&delegator_public_key));

                debug!("pruning delegator bid {}", delegator_bid_addr);
                self.prune_bid(delegator_bid_addr)
            }
            debug!("pruning validator bid {}", validator_bid_addr);
            self.prune_bid(validator_bid_addr);
        } else {
            self.write_bid(validator_bid_key, BidKind::Validator(validator_bid))?;
        }
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
        max_delegators_per_validator: Option<u32>,
        minimum_delegation_amount: u64,
    ) -> Result<U512, ApiError> {
        if !self.allow_auction_bids() {
            // Validation set rotation might be disabled on some private chains and we should not
            // allow new bids to come in.
            return Err(Error::AuctionBidsDisabled.into());
        }

        if !self.is_allowed_session_caller(&AccountHash::from(&delegator_public_key)) {
            return Err(Error::InvalidContext.into());
        }

        let source = self.get_main_purse()?;

        detail::handle_delegation(
            self,
            delegator_public_key,
            validator_public_key,
            source,
            amount,
            max_delegators_per_validator,
            minimum_delegation_amount,
        )
    }

    /// Unbonds aka reduces stake by specified amount, adding an entry to the unbonding queue
    ///
    /// The arguments are the delegator's key, the validator's key, and the amount.
    ///
    /// Returns the remaining staked amount (we allow partial unbonding).
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

        let validator_bid_key = BidAddr::from(validator_public_key.clone()).into();
        let _ = read_validator_bid(self, &validator_bid_key)?;

        let delegator_bid_addr =
            BidAddr::new_from_public_keys(&validator_public_key, Some(&delegator_public_key));
        let mut delegator_bid = read_delegator_bid(self, &delegator_bid_addr.into())?;

        // An attempt to unbond more than is staked results in unbonding the staked amount.
        let unbonding_amount = U512::min(amount, delegator_bid.staked_amount());

        let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;
        let updated_stake =
            delegator_bid.decrease_stake(unbonding_amount, era_end_timestamp_millis)?;

        detail::create_unbonding_purse(
            self,
            validator_public_key,
            delegator_public_key,
            *delegator_bid.bonding_purse(),
            unbonding_amount,
            None,
        )?;

        debug!(
            "undelegation for {} reducing {} by {} to {}",
            delegator_bid_addr,
            delegator_bid.staked_amount(),
            unbonding_amount,
            updated_stake
        );

        if updated_stake.is_zero() {
            debug!("pruning delegator bid {}", delegator_bid_addr);
            self.prune_bid(delegator_bid_addr);
        } else {
            self.write_bid(delegator_bid_addr.into(), BidKind::Delegator(delegator_bid))?;
        }
        Ok(updated_stake)
    }

    /// Unbonds aka reduces stake by specified amount, adding an entry to the unbonding queue,
    /// which when processed will attempt to re-delegate the stake to the specified new validator.
    /// If this is not possible at that future point in time, the unbonded stake will instead
    /// downgrade to a standard undelegate operation automatically (the unbonded stake is
    /// returned to the associated purse).
    ///
    /// This is a quality of life / convenience method, allowing a delegator to indicate they
    /// would like some or all of their stake moved away from a validator to a different validator
    /// with a single transaction, instead of requiring them to send an unbonding transaction
    /// to unbond from the first validator and then wait a number of eras equal to the unbonding
    /// delay and then send a second transaction to bond to the second validator.
    ///
    /// The arguments are the delegator's key, the existing validator's key, the amount,
    /// and the new validator's key.
    ///
    /// Returns the remaining staked amount (we allow partial unbonding).
    fn redelegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
        new_validator: PublicKey,
        minimum_delegation_amount: u64,
    ) -> Result<U512, Error> {
        let delegator_account_hash =
            AccountHash::from_public_key(&delegator_public_key, |x| self.blake2b(x));

        if !self.is_allowed_session_caller(&delegator_account_hash) {
            return Err(Error::InvalidContext);
        }

        if amount < U512::from(minimum_delegation_amount) {
            return Err(Error::DelegationAmountTooSmall);
        }

        // does the validator being moved away from exist?
        let validator_addr = BidAddr::from(validator_public_key.clone());
        let _ = read_validator_bid(self, &validator_addr.into())?;

        let delegator_bid_addr =
            BidAddr::new_from_public_keys(&validator_public_key, Some(&delegator_public_key));

        let mut delegator_bid = read_delegator_bid(self, &delegator_bid_addr.into())?;

        // An attempt to unbond more than is staked results in unbonding the staked amount.
        let unbonding_amount = U512::min(amount, delegator_bid.staked_amount());

        let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;
        let updated_stake =
            delegator_bid.decrease_stake(unbonding_amount, era_end_timestamp_millis)?;

        detail::create_unbonding_purse(
            self,
            validator_public_key,
            delegator_public_key,
            *delegator_bid.bonding_purse(),
            unbonding_amount,
            Some(new_validator),
        )?;

        debug!(
            "redelegation for {} reducing {} by {} to {}",
            delegator_bid_addr,
            delegator_bid.staked_amount(),
            unbonding_amount,
            updated_stake
        );

        if updated_stake.is_zero() {
            debug!("pruning redelegator bid {}", delegator_bid_addr);
            self.prune_bid(delegator_bid_addr);
        } else {
            self.write_bid(delegator_bid_addr.into(), BidKind::Delegator(delegator_bid))?;
        }

        Ok(updated_stake)
    }

    /// Slashes each validator.
    ///
    /// This can be only invoked through a system call.
    fn slash(&mut self, validator_public_keys: Vec<PublicKey>) -> Result<(), Error> {
        fn slash_unbonds(
            validator_public_key: &PublicKey,
            unbonding_purses: Vec<UnbondingPurse>,
        ) -> (U512, Vec<UnbondingPurse>) {
            let mut burned_amount = U512::zero();
            let mut new_unbonding_purses: Vec<UnbondingPurse> = vec![];
            for unbonding_purse in unbonding_purses {
                if unbonding_purse.validator_public_key() != validator_public_key {
                    new_unbonding_purses.push(unbonding_purse);
                    continue;
                }
                burned_amount += *unbonding_purse.amount();
            }
            (burned_amount, new_unbonding_purses)
        }

        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller);
        }

        let mut burned_amount: U512 = U512::zero();

        for validator_public_key in validator_public_keys {
            let validator_bid_addr = BidAddr::from(validator_public_key.clone());
            // Burn stake, deactivate
            if let Some(BidKind::Validator(validator_bid)) =
                self.read_bid(&validator_bid_addr.into())?
            {
                burned_amount += validator_bid.staked_amount();
                self.prune_bid(validator_bid_addr);

                // Also slash delegator stakes when deactivating validator bid.
                let prefix = validator_bid_addr.delegators_prefix()?;
                let delegator_keys = self.get_keys_by_prefix(&prefix)?;
                for delegator_key in delegator_keys {
                    if let Some(BidKind::Delegator(delegator_bid)) =
                        self.read_bid(&delegator_key)?
                    {
                        burned_amount += delegator_bid.staked_amount();
                        let delegator_bid_addr = BidAddr::new_from_public_keys(
                            &validator_public_key,
                            Some(delegator_bid.delegator_public_key()),
                        );
                        self.prune_bid(delegator_bid_addr);

                        let unbonding_purses = self.read_unbonds(&AccountHash::from(
                            delegator_bid.delegator_public_key(),
                        ))?;
                        if unbonding_purses.is_empty() {
                            continue;
                        }
                        let (burned, remaining) =
                            slash_unbonds(&validator_public_key, unbonding_purses);
                        burned_amount += burned;
                        self.write_unbonds(
                            AccountHash::from(&delegator_bid.delegator_public_key().clone()),
                            remaining,
                        )?;
                    }
                }
            }

            // Find any unbonding entries for given validator
            let unbonding_purses = self.read_unbonds(&AccountHash::from(&validator_public_key))?;
            if unbonding_purses.is_empty() {
                continue;
            }
            // get rid of any staked token in the unbonding queue
            let (burned, remaining) = slash_unbonds(&validator_public_key, unbonding_purses);
            burned_amount += burned;
            self.write_unbonds(AccountHash::from(&validator_public_key.clone()), remaining)?;
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
        max_delegators_per_validator: Option<u32>,
        minimum_delegation_amount: u64,
    ) -> Result<(), ApiError> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller.into());
        }

        let vesting_schedule_period_millis = self.vesting_schedule_period_millis();
        let validator_slots = detail::get_validator_slots(self)?;
        let auction_delay = detail::get_auction_delay(self)?;
        let snapshot_size = auction_delay as usize + 1;
        let mut era_id: EraId = detail::get_era_id(self)?;

        // Process unbond requests
        detail::process_unbond_requests(
            self,
            max_delegators_per_validator,
            minimum_delegation_amount,
        )?;

        let mut validator_bids = detail::get_validator_bids(self)?;

        // Process bids
        let mut bids_modified = false;
        for (validator_public_key, validator_bid) in validator_bids.iter_mut() {
            if process_with_vesting_schedule(
                self,
                validator_bid,
                era_end_timestamp_millis,
                self.vesting_schedule_period_millis(),
            )? {
                bids_modified = true;
            }

            if evicted_validators.contains(validator_public_key) {
                bids_modified = validator_bid.deactivate();
            }
        }

        // Compute next auction winners
        let winners: ValidatorWeights = {
            let locked_validators: ValidatorWeights = validator_bids
                .iter()
                .filter(|(_public_key, bid)| {
                    bid.is_locked_with_vesting_schedule(
                        era_end_timestamp_millis,
                        vesting_schedule_period_millis,
                    ) && !bid.inactive()
                })
                .map(|(public_key, bid)| {
                    let total_staked_amount = detail::total_staked_amount(self, bid)?;
                    Ok((public_key.clone(), total_staked_amount))
                })
                .collect::<Result<ValidatorWeights, Error>>()?;

            // We collect these into a vec for sorting
            let mut unlocked_validators: Vec<(PublicKey, U512)> = validator_bids
                .iter()
                .filter(|(_public_key, bid)| {
                    !bid.is_locked_with_vesting_schedule(
                        era_end_timestamp_millis,
                        vesting_schedule_period_millis,
                    ) && !bid.inactive()
                })
                .map(|(public_key, validator_bid)| {
                    let total_staked_amount = detail::total_staked_amount(self, validator_bid)?;
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
                let seigniorage_recipient = match validator_bids.get(era_validator) {
                    Some(validator_bid) => seigniorage_recipient(self, validator_bid)?,
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
            detail::set_validator_bids(self, validator_bids)?;
        }

        Ok(())
    }

    /// Mint and distribute seigniorage rewards to validators and their delegators,
    /// according to `reward_factors` returned by the consensus component.
    // TODO: rework EraInfo and other related structs, methods, etc. to report correct era-end
    // totals of per-block rewards
    fn distribute(&mut self, rewards: BTreeMap<PublicKey, U512>) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller);
        }

        let seigniorage_recipients = self.read_seigniorage_recipients()?;
        let mut era_info = EraInfo::new();
        let seigniorage_allocations = era_info.seigniorage_allocations_mut();

        for (proposer, reward_amount) in rewards
            .into_iter()
            .filter(|(key, _amount)| key != &PublicKey::System)
        {
            let total_reward = Ratio::from(reward_amount);
            let recipient = seigniorage_recipients
                .get(&proposer)
                .ok_or(Error::ValidatorNotFound)?;

            let total_stake = recipient.total_stake().ok_or(Error::ArithmeticOverflow)?;
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

            let delegator_payouts = detail::distribute_delegator_rewards(
                self,
                seigniorage_allocations,
                proposer.clone(),
                delegator_rewards,
            )?;

            let total_delegator_payout: U512 = delegator_payouts
                .iter()
                .map(|(_delegator_hash, amount, _bonding_purse)| *amount)
                .sum();

            let validator_reward = reward_amount - total_delegator_payout;
            let validator_bonding_purse = detail::distribute_validator_rewards(
                self,
                seigniorage_allocations,
                proposer.clone(),
                validator_reward,
            )?;

            // mint new token and put it to the recipients' purses
            self.mint_into_existing_purse(validator_reward, validator_bonding_purse)
                .map_err(Error::from)?;

            for (_delegator_account_hash, delegator_payout, bonding_purse) in delegator_payouts {
                self.mint_into_existing_purse(delegator_payout, bonding_purse)
                    .map_err(Error::from)?;
            }
        }

        // record allocations for this era for reporting purposes.
        self.record_era_info(era_info)?;

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

        let key = BidAddr::from(validator_public_key).into();
        if let Some(BidKind::Validator(mut validator_bid)) = self.read_bid(&key)? {
            validator_bid.activate();
            self.write_bid(key, BidKind::Validator(validator_bid))?;
            Ok(())
        } else {
            Err(Error::ValidatorNotFound)
        }
    }
}
