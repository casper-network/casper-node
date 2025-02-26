mod auction_native;
/// Auction business logic.
pub mod detail;
/// System logic providers.
pub mod providers;

use std::collections::BTreeMap;

use itertools::Itertools;
use num_rational::Ratio;
use num_traits::{CheckedMul, CheckedSub};
use tracing::{debug, error, warn};

use crate::system::auction::detail::{
    process_with_vesting_schedule, read_delegator_bid, read_delegator_bids, read_validator_bid,
    seigniorage_recipients,
};
use casper_types::{
    account::AccountHash,
    system::auction::{
        BidAddr, BidKind, Bridge, DelegationRate, DelegatorKind, EraInfo, EraValidators, Error,
        Reservation, SeigniorageRecipient, SeigniorageRecipientsSnapshot, SeigniorageRecipientsV2,
        UnbondEra, UnbondKind, ValidatorBid, ValidatorCredit, ValidatorWeights,
        DELEGATION_RATE_DENOMINATOR,
    },
    AccessRights, ApiError, EraId, Key, PublicKey, URef, U512,
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
    fn read_seigniorage_recipients(&mut self) -> Result<SeigniorageRecipientsV2, Error> {
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
    #[allow(clippy::too_many_arguments)]
    fn add_bid(
        &mut self,
        public_key: PublicKey,
        delegation_rate: DelegationRate,
        amount: U512,
        minimum_delegation_amount: u64,
        maximum_delegation_amount: u64,
        minimum_bid_amount: u64,
        max_delegators_per_validator: u32,
        reserved_slots: u32,
    ) -> Result<U512, ApiError> {
        if !self.allow_auction_bids() {
            // The validator set may be closed on some side chains,
            // which is configured by disabling bids.
            return Err(Error::AuctionBidsDisabled.into());
        }

        if amount < U512::from(minimum_bid_amount) {
            return Err(Error::BondTooSmall.into());
        }

        if delegation_rate > DELEGATION_RATE_DENOMINATOR {
            return Err(Error::DelegationRateTooLarge.into());
        }

        if reserved_slots > max_delegators_per_validator {
            return Err(Error::ExceededReservationSlotsLimit.into());
        }

        let provided_account_hash = AccountHash::from(&public_key);

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext.into());
        }

        let validator_bid_key = BidAddr::from(public_key.clone()).into();
        let (target, validator_bid) = if let Some(BidKind::Validator(mut validator_bid)) =
            self.read_bid(&validator_bid_key)?
        {
            // update an existing validator bid
            if validator_bid.inactive() {
                validator_bid.activate();
            }
            validator_bid.increase_stake(amount)?;
            validator_bid.with_delegation_rate(delegation_rate);
            validator_bid.set_delegation_amount_boundaries(
                minimum_delegation_amount,
                maximum_delegation_amount,
            );
            // perform validation if number of reserved slots has changed
            if reserved_slots != validator_bid.reserved_slots() {
                let validator_bid_addr = BidAddr::from(public_key.clone());
                // cannot reserve fewer slots than there are reservations
                let reservation_count = self.reservation_count(&validator_bid_addr)?;
                if reserved_slots < reservation_count as u32 {
                    return Err(Error::ReservationSlotsCountTooSmall.into());
                }

                // cannot reserve more slots than there are free delegator slots
                let used_reservation_count = self.used_reservation_count(&validator_bid_addr)?;
                let delegator_count = self.delegator_count(&validator_bid_addr)?;
                let normal_delegators = delegator_count - used_reservation_count;
                let max_reserved_slots = max_delegators_per_validator - normal_delegators as u32;
                if reserved_slots > max_reserved_slots {
                    return Err(Error::ExceededReservationSlotsLimit.into());
                }
                validator_bid.with_reserved_slots(reserved_slots);
            }

            (*validator_bid.bonding_purse(), validator_bid)
        } else {
            // create new validator bid
            let bonding_purse = self.create_purse()?;
            let validator_bid = ValidatorBid::unlocked(
                public_key,
                bonding_purse,
                amount,
                delegation_rate,
                minimum_delegation_amount,
                maximum_delegation_amount,
                reserved_slots,
            );
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
            // Propagate mint contract's error that occurred during execution of transfer
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
    fn withdraw_bid(
        &mut self,
        public_key: PublicKey,
        amount: U512,
        minimum_bid_amount: u64,
    ) -> Result<U512, Error> {
        let provided_account_hash = AccountHash::from(&public_key);

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext);
        }

        let validator_bid_addr = BidAddr::from(public_key.clone());
        let validator_bid_key = validator_bid_addr.into();
        let mut validator_bid = read_validator_bid(self, &validator_bid_key)?;
        let staked_amount = validator_bid.staked_amount();

        // An attempt to unbond more than is staked results in unbonding the staked amount.
        let unbonding_amount = U512::min(amount, validator_bid.staked_amount());

        let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;
        let updated_stake =
            validator_bid.decrease_stake(unbonding_amount, era_end_timestamp_millis)?;

        debug!(
            "withdrawing bid for {validator_bid_addr} reducing {staked_amount} by {unbonding_amount} to {updated_stake}",
        );
        // if validator stake is less than minimum_bid_amount, unbond fully and prune validator bid
        if updated_stake < U512::from(minimum_bid_amount) {
            // create unbonding purse for full validator stake
            detail::create_unbonding_purse(
                self,
                public_key.clone(),
                UnbondKind::Validator(public_key.clone()), // validator is the unbonder
                *validator_bid.bonding_purse(),
                staked_amount,
                None,
            )?;
            // Unbond all delegators and zero them out
            let delegators = read_delegator_bids(self, &public_key)?;
            for mut delegator in delegators {
                let unbond_kind = delegator.unbond_kind();
                detail::create_unbonding_purse(
                    self,
                    public_key.clone(),
                    unbond_kind,
                    *delegator.bonding_purse(),
                    delegator.staked_amount(),
                    None,
                )?;
                delegator.decrease_stake(delegator.staked_amount(), era_end_timestamp_millis)?;

                let delegator_bid_addr = delegator.bid_addr();
                debug!("pruning delegator bid {}", delegator_bid_addr);
                self.prune_bid(delegator_bid_addr)
            }
            debug!("pruning validator bid {}", validator_bid_addr);
            self.prune_bid(validator_bid_addr);
        } else {
            // create unbonding purse for the unbonding amount
            detail::create_unbonding_purse(
                self,
                public_key.clone(),
                UnbondKind::Validator(public_key.clone()), // validator is the unbonder
                *validator_bid.bonding_purse(),
                unbonding_amount,
                None,
            )?;
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
        delegator_kind: DelegatorKind,
        validator_public_key: PublicKey,
        amount: U512,
        max_delegators_per_validator: u32,
    ) -> Result<U512, ApiError> {
        if !self.allow_auction_bids() {
            // Validation set rotation might be disabled on some private chains and we should not
            // allow new bids to come in.
            return Err(Error::AuctionBidsDisabled.into());
        }

        let source = match &delegator_kind {
            DelegatorKind::PublicKey(pk) => {
                let account_hash = pk.to_account_hash();
                if !self.is_allowed_session_caller(&account_hash) {
                    return Err(Error::InvalidContext.into());
                }
                self.get_main_purse()?
            }
            DelegatorKind::Purse(addr) => {
                let uref = URef::new(*addr, AccessRights::WRITE);
                if !self.is_valid_uref(uref) {
                    return Err(Error::InvalidContext.into());
                }
                uref
            }
        };

        detail::handle_delegation(
            self,
            delegator_kind,
            validator_public_key,
            source,
            amount,
            max_delegators_per_validator,
        )
    }

    /// Unbonds aka reduces stake by specified amount, adding an entry to the unbonding queue
    ///
    /// The arguments are the delegator's key, the validator's key, and the amount.
    ///
    /// Returns the remaining staked amount (we allow partial unbonding).
    fn undelegate(
        &mut self,
        delegator_kind: DelegatorKind,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Result<U512, Error> {
        match &delegator_kind {
            DelegatorKind::PublicKey(pk) => {
                let account_hash = pk.to_account_hash();
                if !self.is_allowed_session_caller(&account_hash) {
                    return Err(Error::InvalidContext);
                }
            }
            DelegatorKind::Purse(addr) => {
                let uref = URef::new(*addr, AccessRights::WRITE);
                if !self.is_valid_uref(uref) {
                    return Err(Error::InvalidContext);
                }
            }
        }

        let validator_bid_key = BidAddr::from(validator_public_key.clone()).into();
        let _ = read_validator_bid(self, &validator_bid_key)?;

        let delegator_bid_addr =
            BidAddr::new_delegator_kind(&validator_public_key, &delegator_kind);
        let mut delegator_bid = read_delegator_bid(self, &delegator_bid_addr.into())?;

        // An attempt to unbond more than is staked results in unbonding the full staked amount.
        let unbonding_amount = U512::min(amount, delegator_bid.staked_amount());

        let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;
        let updated_stake =
            delegator_bid.decrease_stake(unbonding_amount, era_end_timestamp_millis)?;

        let unbond_kind = delegator_kind.into();

        detail::create_unbonding_purse(
            self,
            validator_public_key,
            unbond_kind,
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
        delegator_kind: DelegatorKind,
        validator_public_key: PublicKey,
        amount: U512,
        new_validator: PublicKey,
    ) -> Result<U512, Error> {
        match &delegator_kind {
            DelegatorKind::PublicKey(pk) => {
                let delegator_account_hash = pk.to_account_hash();
                if !self.is_allowed_session_caller(&delegator_account_hash) {
                    return Err(Error::InvalidContext);
                }
            }
            DelegatorKind::Purse(addr) => {
                if !self.is_valid_uref(URef::new(*addr, AccessRights::WRITE)) {
                    return Err(Error::InvalidContext);
                }
            }
        }

        // does the validator being moved away from exist?
        let validator_addr = BidAddr::from(validator_public_key.clone());
        let validator_bid = read_validator_bid(self, &validator_addr.into())?;
        if amount < U512::from(validator_bid.minimum_delegation_amount()) {
            return Err(Error::DelegationAmountTooSmall);
        }
        if amount > U512::from(validator_bid.maximum_delegation_amount()) {
            return Err(Error::DelegationAmountTooLarge);
        }

        let delegator_bid_addr =
            BidAddr::new_delegator_kind(&validator_public_key, &delegator_kind);

        let mut delegator_bid = read_delegator_bid(self, &delegator_bid_addr.into())?;

        // An attempt to unbond more than is staked results in unbonding the staked amount.
        let unbonding_amount = U512::min(amount, delegator_bid.staked_amount());

        let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;
        let updated_stake =
            delegator_bid.decrease_stake(unbonding_amount, era_end_timestamp_millis)?;

        detail::create_unbonding_purse(
            self,
            validator_public_key,
            delegator_kind.into(),
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

    /// Unbond delegator bids which fall outside validator-configured delegation limits.
    fn forced_undelegate(&mut self) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller);
        }

        let era_end_timestamp_millis = detail::get_era_end_timestamp_millis(self)?;
        let era_id = detail::get_era_id(self)?;
        let bids_detail = detail::get_validator_bids(self, era_id)?;

        // Forcibly undelegate bids outside a validator's delegation limits
        for (validator_public_key, validator_bid) in bids_detail.validator_bids().iter() {
            let minimum_delegation_amount = U512::from(validator_bid.minimum_delegation_amount());
            let maximum_delegation_amount = U512::from(validator_bid.maximum_delegation_amount());

            let delegators = read_delegator_bids(self, validator_public_key)?;
            for mut delegator in delegators {
                let staked_amount = delegator.staked_amount();
                let delegator_bid_addr = delegator.bid_addr();
                if staked_amount.is_zero() {
                    // If the stake is zero, we don't need to unbond anything - we can just prune
                    // the bid outright. Also, we don't want to keep zero bids even if the minimum
                    // allowed amount is zero, as they only use up space for no reason.
                    debug!("pruning delegator bid {}", delegator_bid_addr);
                    self.prune_bid(delegator_bid_addr);
                } else if staked_amount < minimum_delegation_amount
                    || staked_amount > maximum_delegation_amount
                {
                    let amount = if staked_amount < minimum_delegation_amount {
                        staked_amount
                    } else {
                        staked_amount - maximum_delegation_amount
                    };
                    let unbond_kind = delegator.unbond_kind();
                    detail::create_unbonding_purse(
                        self,
                        validator_public_key.clone(),
                        unbond_kind,
                        *delegator.bonding_purse(),
                        amount,
                        None,
                    )?;
                    let updated_stake =
                        match delegator.decrease_stake(amount, era_end_timestamp_millis) {
                            Ok(updated_stake) => updated_stake,
                            // Work around the case when the locked amounts table has yet to be
                            // initialized (likely pre-90 day mark).
                            Err(Error::DelegatorFundsLocked) => continue,
                            Err(err) => return Err(err),
                        };

                    if updated_stake.is_zero() {
                        debug!("pruning delegator bid {}", delegator_bid_addr);
                        self.prune_bid(delegator_bid_addr);
                    } else {
                        debug!(
                            "forced undelegation for {} reducing {} by {} to {}",
                            delegator_bid_addr, staked_amount, amount, updated_stake
                        );
                        self.write_bid(
                            delegator_bid_addr.into(),
                            BidKind::Delegator(Box::new(delegator)),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Adds new reservations for a given validator with specified delegator public keys
    /// and delegation rates. If during adding reservations configured number of reserved
    /// delegator slots is exceeded it returns an error.
    ///
    /// If given reservation exists already and the delegation rate was changed it's updated.
    fn add_reservations(&mut self, reservations: Vec<Reservation>) -> Result<(), Error> {
        if !self.allow_auction_bids() {
            // Validation set rotation might be disabled on some private chains and we should not
            // allow new bids to come in.
            return Err(Error::AuctionBidsDisabled);
        }

        for reservation in reservations {
            if !self
                .is_allowed_session_caller(&AccountHash::from(reservation.validator_public_key()))
            {
                return Err(Error::InvalidContext);
            }

            detail::handle_add_reservation(self, reservation)?;
        }
        Ok(())
    }

    /// Removes reservations for given delegator public keys. If a reservation for one of the keys
    /// does not exist it returns an error.
    fn cancel_reservations(
        &mut self,
        validator: PublicKey,
        delegators: Vec<DelegatorKind>,
        max_delegators_per_validator: u32,
    ) -> Result<(), Error> {
        if !self.is_allowed_session_caller(&AccountHash::from(&validator)) {
            return Err(Error::InvalidContext);
        }

        for delegator in delegators {
            detail::handle_cancel_reservation(
                self,
                validator.clone(),
                delegator.clone(),
                max_delegators_per_validator,
            )?;
        }
        Ok(())
    }

    /// Slashes each validator.
    ///
    /// This can be only invoked through a system call.
    fn slash(&mut self, validator_public_keys: Vec<PublicKey>) -> Result<(), Error> {
        fn slash_unbonds(unbond_eras: Vec<UnbondEra>) -> U512 {
            let mut burned_amount = U512::zero();
            for unbond_era in unbond_eras {
                burned_amount += *unbond_era.amount();
            }
            burned_amount
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
                let delegator_keys = {
                    let mut ret =
                        self.get_keys_by_prefix(&validator_bid_addr.delegated_account_prefix()?)?;
                    ret.extend(
                        self.get_keys_by_prefix(&validator_bid_addr.delegated_purse_prefix()?)?,
                    );
                    ret
                };

                for delegator_key in delegator_keys {
                    if let Some(BidKind::Delegator(delegator_bid)) =
                        self.read_bid(&delegator_key)?
                    {
                        burned_amount += delegator_bid.staked_amount();
                        let delegator_bid_addr = delegator_bid.bid_addr();
                        self.prune_bid(delegator_bid_addr);

                        // Also slash delegator unbonds.
                        let delegator_unbond_addr = match delegator_bid.delegator_kind() {
                            DelegatorKind::PublicKey(pk) => BidAddr::UnbondAccount {
                                validator: validator_public_key.to_account_hash(),
                                unbonder: pk.to_account_hash(),
                            },
                            DelegatorKind::Purse(addr) => BidAddr::UnbondPurse {
                                validator: validator_public_key.to_account_hash(),
                                unbonder: *addr,
                            },
                        };

                        match self.read_unbond(delegator_unbond_addr)? {
                            Some(unbond) => {
                                let burned = slash_unbonds(unbond.take_eras());

                                burned_amount += burned;
                                self.write_unbond(delegator_unbond_addr, None)?;
                            }
                            None => {
                                continue;
                            }
                        }
                    }
                }
            }

            // get rid of any staked token in the unbonding queue
            let validator_unbond_addr = BidAddr::UnbondAccount {
                validator: validator_public_key.to_account_hash(),
                unbonder: validator_public_key.to_account_hash(),
            };
            match self.read_unbond(validator_unbond_addr)? {
                Some(unbond) => {
                    let burned = slash_unbonds(unbond.take_eras());
                    burned_amount += burned;
                    self.write_unbond(validator_unbond_addr, None)?;
                }
                None => {
                    continue;
                }
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
        max_delegators_per_validator: u32,
        include_credits: bool,
        credit_cap: Ratio<U512>,
    ) -> Result<(), ApiError> {
        debug!("run_auction called");

        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller.into());
        }

        let vesting_schedule_period_millis = self.vesting_schedule_period_millis();
        let validator_slots = detail::get_validator_slots(self)?;
        let auction_delay = detail::get_auction_delay(self)?;
        // We have to store auction_delay future eras, one current era and one past era (for
        // rewards calculations).
        let snapshot_size = auction_delay as usize + 2;
        let mut era_id: EraId = detail::get_era_id(self)?;

        // Process unbond requests
        debug!("processing unbond requests");
        detail::process_unbond_requests(self, max_delegators_per_validator)?;
        debug!("processing unbond request successful");

        let mut validator_bids_detail = detail::get_validator_bids(self, era_id)?;

        // Process bids
        let mut bids_modified = false;
        for (validator_public_key, validator_bid) in
            validator_bids_detail.validator_bids_mut().iter_mut()
        {
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
            let locked_validators = validator_bids_detail.validator_weights(
                era_id,
                era_end_timestamp_millis,
                vesting_schedule_period_millis,
                true,
                include_credits,
                credit_cap,
            )?;

            let remaining_auction_slots = validator_slots.saturating_sub(locked_validators.len());
            if remaining_auction_slots > 0 {
                let unlocked_validators = validator_bids_detail.validator_weights(
                    era_id,
                    era_end_timestamp_millis,
                    vesting_schedule_period_millis,
                    false,
                    include_credits,
                    credit_cap,
                )?;
                let mut unlocked_validators = unlocked_validators
                    .iter()
                    .map(|(public_key, validator_bid)| (public_key.clone(), *validator_bid))
                    .collect::<Vec<(PublicKey, U512)>>();

                unlocked_validators.sort_by(|(_, lhs), (_, rhs)| rhs.cmp(lhs));
                locked_validators
                    .into_iter()
                    .chain(
                        unlocked_validators
                            .into_iter()
                            .take(remaining_auction_slots),
                    )
                    .collect()
            } else {
                locked_validators
            }
        };

        let (validator_bids, validator_credits, delegator_bids, reservations) =
            validator_bids_detail.destructure();

        // call prune BEFORE incrementing the era
        detail::prune_validator_credits(self, era_id, &validator_credits);

        // Increment era
        era_id = era_id.checked_add(1).ok_or(Error::ArithmeticOverflow)?;

        let delayed_era = era_id
            .checked_add(auction_delay)
            .ok_or(Error::ArithmeticOverflow)?;

        // Update seigniorage recipients for current era
        {
            let mut snapshot = detail::get_seigniorage_recipients_snapshot(self)?;
            let recipients =
                seigniorage_recipients(&winners, &validator_bids, &delegator_bids, &reservations)?;
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

        debug!("run_auction successful");

        Ok(())
    }

    /// Mint and distribute seigniorage rewards to validators and their delegators,
    /// according to `reward_factors` returned by the consensus component.
    // TODO: rework EraInfo and other related structs, methods, etc. to report correct era-end
    // totals of per-block rewards
    fn distribute(&mut self, rewards: BTreeMap<PublicKey, Vec<U512>>) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            error!("invalid caller to auction distribute");
            return Err(Error::InvalidCaller);
        }

        debug!("reading seigniorage recipients snapshot");
        let seigniorage_recipients_snapshot = detail::get_seigniorage_recipients_snapshot(self)?;
        let current_era_id = detail::get_era_id(self)?;

        let mut era_info = EraInfo::new();
        let seigniorage_allocations = era_info.seigniorage_allocations_mut();

        debug!(rewards_set_size = rewards.len(), "processing rewards");
        for item in rewards
            .into_iter()
            .filter(|(key, _amounts)| key != &PublicKey::System)
            .map(|(proposer, amounts)| {
                rewards_per_validator(
                    &proposer,
                    current_era_id,
                    &amounts,
                    &SeigniorageRecipientsSnapshot::V2(seigniorage_recipients_snapshot.clone()),
                )
                .map(|infos| infos.into_iter().map(move |info| (proposer.clone(), info)))
            })
            .flatten_ok()
        {
            let (proposer, reward_info) = item?;

            // fetch most recent validator public key if public key was changed
            // or the validator withdrew their bid completely
            let validator_public_key =
                match detail::get_most_recent_validator_public_key(self, proposer.clone()) {
                    Ok(pubkey) => pubkey,
                    Err(Error::BridgeRecordChainTooLong) => {
                        // Validator bid's public key has been changed too many times,
                        // and we were unable to find the current public key.
                        // In this case we are unable to distribute rewards for this validator.
                        continue;
                    }
                    Err(err) => return Err(err),
                };

            debug!(?validator_public_key, "delegator payout for validator");
            let delegator_payouts = detail::distribute_delegator_rewards(
                self,
                seigniorage_allocations,
                validator_public_key.clone(),
                reward_info.delegator_rewards,
            )?;
            debug!(
                ?validator_public_key,
                delegator_set_size = delegator_payouts.len(),
                "delegator payout finished"
            );

            let validator_bonding_purse = detail::distribute_validator_rewards(
                self,
                seigniorage_allocations,
                validator_public_key.clone(),
                reward_info.validator_reward,
            )?;
            debug!(?validator_public_key, "validator payout finished");

            // mint new token and put it to the recipients' purses
            self.mint_into_existing_purse(reward_info.validator_reward, validator_bonding_purse)?;

            for (_delegator_account_hash, delegator_payout, bonding_purse) in delegator_payouts {
                self.mint_into_existing_purse(delegator_payout, bonding_purse)?;
            }
            debug!("rewards minted into recipient purses");
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
    fn activate_bid(&mut self, validator: PublicKey) -> Result<(), Error> {
        let provided_account_hash = AccountHash::from(&validator);

        if !self.is_allowed_session_caller(&provided_account_hash) {
            return Err(Error::InvalidContext);
        }

        let key = BidAddr::from(validator).into();
        if let Some(BidKind::Validator(mut validator_bid)) = self.read_bid(&key)? {
            validator_bid.activate();
            self.write_bid(key, BidKind::Validator(validator_bid))?;
            Ok(())
        } else {
            Err(Error::ValidatorNotFound)
        }
    }

    /// Updates a `ValidatorBid` and all related delegator bids to use a new public key.
    ///
    /// This in effect "transfers" a validator bid along with its stake and all delegators
    /// from one public key to another.
    /// This method can only be called by the account associated with the current `ValidatorBid`.
    ///
    /// The arguments are the existing bid's 'validator_public_key' and the new public key.
    fn change_bid_public_key(
        &mut self,
        public_key: PublicKey,
        new_public_key: PublicKey,
    ) -> Result<(), Error> {
        let validator_account_hash = AccountHash::from(&public_key);

        // check that the caller is the current bid's owner
        if !self.is_allowed_session_caller(&validator_account_hash) {
            return Err(Error::InvalidContext);
        }

        // verify that a bid for given public key exists
        let validator_bid_addr = BidAddr::from(public_key.clone());
        let mut validator_bid = read_validator_bid(self, &validator_bid_addr.into())?;

        // verify that a bid for the new key does not exist yet
        let new_validator_bid_addr = BidAddr::from(new_public_key.clone());
        if self.read_bid(&new_validator_bid_addr.into())?.is_some() {
            return Err(Error::ValidatorBidExistsAlready);
        }

        debug!("changing validator bid {validator_bid_addr} public key from {public_key} to {new_public_key}");

        // store new validator bid
        validator_bid.with_validator_public_key(new_public_key.clone());
        self.write_bid(
            new_validator_bid_addr.into(),
            BidKind::Validator(validator_bid),
        )?;

        // store bridge record in place of old validator bid
        let bridge = Bridge::new(
            public_key.clone(),
            new_public_key.clone(),
            self.read_era_id()?,
        );
        self.write_bid(validator_bid_addr.into(), BidKind::Bridge(Box::new(bridge)))?;

        debug!("transferring delegator bids from validator bid {validator_bid_addr} to {new_validator_bid_addr}");
        let delegators = read_delegator_bids(self, &public_key)?;
        for mut delegator in delegators {
            let delegator_bid_addr =
                BidAddr::new_delegator_kind(&public_key, delegator.delegator_kind());

            delegator.with_validator_public_key(new_public_key.clone());
            let new_delegator_bid_addr =
                BidAddr::new_delegator_kind(&new_public_key, delegator.delegator_kind());

            self.write_bid(
                new_delegator_bid_addr.into(),
                BidKind::Delegator(Box::from(delegator)),
            )?;

            debug!("pruning delegator bid {delegator_bid_addr}");
            self.prune_bid(delegator_bid_addr);
        }

        Ok(())
    }

    /// Writes a validator credit record.
    fn write_validator_credit(
        &mut self,
        validator: PublicKey,
        era_id: EraId,
        amount: U512,
    ) -> Result<Option<BidAddr>, Error> {
        // only the system may use this method
        if self.get_caller() != PublicKey::System.to_account_hash() {
            error!("invalid caller to auction validator_credit");
            return Err(Error::InvalidCaller);
        }

        // is imputed public key associated with a validator bid record?
        let bid_addr = BidAddr::new_from_public_keys(&validator, None);
        let key = Key::BidAddr(bid_addr);
        let _ = match self.read_bid(&key)? {
            Some(bid_kind) => bid_kind,
            None => {
                warn!(
                    ?key,
                    ?era_id,
                    ?amount,
                    "attempt to add a validator credit to a non-existent validator"
                );
                return Ok(None);
            }
        };

        // if amount is zero, noop
        if amount.is_zero() {
            return Ok(None);
        }

        // write credit record
        let credit_addr = BidAddr::new_credit(&validator, era_id);
        let credit_key = Key::BidAddr(credit_addr);
        let credit_bid = match self.read_bid(&credit_key)? {
            Some(BidKind::Credit(mut existing_credit)) => {
                existing_credit.increase(amount);
                existing_credit
            }
            Some(_) => return Err(Error::UnexpectedBidVariant),
            None => Box::new(ValidatorCredit::new(validator, era_id, amount)),
        };

        self.write_bid(credit_key, BidKind::Credit(credit_bid))
            .map(|_| Some(credit_addr))
    }
}

/// Retrieves the total reward for a given validator or delegator in a given era.
pub fn reward(
    validator: &PublicKey,
    delegator: Option<&DelegatorKind>,
    era_id: EraId,
    rewards: &[U512],
    seigniorage_recipients_snapshot: &SeigniorageRecipientsSnapshot,
) -> Result<Option<U512>, Error> {
    let validator_rewards =
        match rewards_per_validator(validator, era_id, rewards, seigniorage_recipients_snapshot) {
            Ok(rewards) => rewards,
            Err(Error::ValidatorNotFound) => return Ok(None),
            Err(Error::MissingSeigniorageRecipients) => return Ok(None),
            Err(err) => return Err(err),
        };

    let reward = validator_rewards
        .into_iter()
        .map(|reward_info| {
            if let Some(delegator) = delegator {
                reward_info
                    .delegator_rewards
                    .get(delegator)
                    .copied()
                    .unwrap_or_default()
            } else {
                reward_info.validator_reward
            }
        })
        .sum();

    Ok(Some(reward))
}

fn rewards_per_validator(
    validator: &PublicKey,
    era_id: EraId,
    rewards: &[U512],
    seigniorage_recipients_snapshot: &SeigniorageRecipientsSnapshot,
) -> Result<Vec<RewardsPerValidator>, Error> {
    let mut results = Vec::with_capacity(rewards.len());

    for (reward_amount, eras_back) in rewards
        .iter()
        .enumerate()
        .map(move |(i, &amount)| (amount, i as u64))
        // do not process zero amounts, unless they are for the current era (we still want to
        // record zero allocations for the current validators in EraInfo)
        .filter(|(amount, eras_back)| !amount.is_zero() || *eras_back == 0)
    {
        let total_reward = Ratio::from(reward_amount);
        let rewarded_era = era_id
            .checked_sub(eras_back)
            .ok_or(Error::MissingSeigniorageRecipients)?;

        // try to find validator in seigniorage snapshot
        let maybe_seigniorage_recipient = match seigniorage_recipients_snapshot {
            SeigniorageRecipientsSnapshot::V1(snapshot) => snapshot
                .get(&rewarded_era)
                .ok_or(Error::MissingSeigniorageRecipients)?
                .get(validator)
                .cloned()
                .map(SeigniorageRecipient::V1),
            SeigniorageRecipientsSnapshot::V2(snapshot) => snapshot
                .get(&rewarded_era)
                .ok_or(Error::MissingSeigniorageRecipients)?
                .get(validator)
                .cloned()
                .map(SeigniorageRecipient::V2),
        };

        let Some(recipient) = maybe_seigniorage_recipient else {
            // We couldn't find the validator. If the reward amount is zero, we don't care -
            // the validator wasn't supposed to be rewarded in this era, anyway. Otherwise,
            // return an error.
            if reward_amount.is_zero() {
                continue;
            } else {
                return Err(Error::ValidatorNotFound);
            }
        };

        let total_stake = recipient.total_stake().ok_or(Error::ArithmeticOverflow)?;

        if total_stake.is_zero() {
            // The validator has completely unbonded. We can't compute the delegators' part (as
            // their stakes are also zero), so we just give the whole reward to the validator.
            // When used from `distribute`, we will mint the reward into their bonding purse
            // and increase their unbond request by the corresponding amount.

            results.push(RewardsPerValidator {
                validator_reward: reward_amount,
                delegator_rewards: BTreeMap::new(),
            });
            continue;
        }

        let delegator_total_stake: U512 = recipient
            .delegator_total_stake()
            .ok_or(Error::ArithmeticOverflow)?;

        // calculate part of reward to be distributed to delegators before commission
        let base_delegators_part: Ratio<U512> = {
            let reward_multiplier: Ratio<U512> = Ratio::new(delegator_total_stake, total_stake);
            total_reward
                .checked_mul(&reward_multiplier)
                .ok_or(Error::ArithmeticOverflow)?
        };

        let default = BTreeMap::new();
        let reservation_delegation_rates =
            recipient.reservation_delegation_rates().unwrap_or(&default);
        // calculate commission and final reward for each delegator
        let mut delegator_rewards: BTreeMap<DelegatorKind, U512> = BTreeMap::new();
        for (delegator_kind, delegator_stake) in recipient.delegator_stake().iter() {
            let reward_multiplier = Ratio::new(*delegator_stake, delegator_total_stake);
            let base_reward = base_delegators_part * reward_multiplier;
            let delegation_rate = *reservation_delegation_rates
                .get(delegator_kind)
                .unwrap_or(recipient.delegation_rate());
            let commission_rate = Ratio::new(
                U512::from(delegation_rate),
                U512::from(DELEGATION_RATE_DENOMINATOR),
            );
            let commission: Ratio<U512> = base_reward
                .checked_mul(&commission_rate)
                .ok_or(Error::ArithmeticOverflow)?;
            let reward = base_reward
                .checked_sub(&commission)
                .ok_or(Error::ArithmeticOverflow)?;
            delegator_rewards.insert(delegator_kind.clone(), reward.to_integer());
        }

        let total_delegator_payout: U512 =
            delegator_rewards.iter().map(|(_, &amount)| amount).sum();

        let validator_reward = reward_amount - total_delegator_payout;

        results.push(RewardsPerValidator {
            validator_reward,
            delegator_rewards,
        });
    }
    Ok(results)
}

/// Aggregated rewards data for a validator.
#[derive(Debug, Default)]
pub struct RewardsPerValidator {
    validator_reward: U512,
    delegator_rewards: BTreeMap<DelegatorKind, U512>,
}
