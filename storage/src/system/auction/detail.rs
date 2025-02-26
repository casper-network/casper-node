use std::{collections::BTreeMap, convert::TryInto, ops::Mul};

use num_rational::Ratio;

use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    system::auction::{
        BidAddr, BidAddrTag, BidKind, DelegatorBid, DelegatorBids, DelegatorKind, Error,
        Reservation, Reservations, SeigniorageAllocation, SeigniorageRecipientV2,
        SeigniorageRecipientsSnapshotV1, SeigniorageRecipientsSnapshotV2, SeigniorageRecipientsV2,
        Unbond, UnbondEra, UnbondKind, ValidatorBid, ValidatorBids, ValidatorCredit,
        ValidatorCredits, AUCTION_DELAY_KEY, DELEGATION_RATE_DENOMINATOR,
        ERA_END_TIMESTAMP_MILLIS_KEY, ERA_ID_KEY, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
        UNBONDING_DELAY_KEY, VALIDATOR_SLOTS_KEY,
    },
    ApiError, CLTyped, EraId, Key, KeyTag, PublicKey, URef, U512,
};
use tracing::{debug, error, warn};

use super::{
    Auction, EraValidators, MintProvider, RuntimeProvider, StorageProvider, ValidatorWeights,
};

/// Maximum length of bridge records chain.
/// Used when looking for the most recent bid record to avoid unbounded computations.
const MAX_BRIDGE_CHAIN_LENGTH: u64 = 20;

fn read_from<P, T>(provider: &mut P, name: &str) -> Result<T, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
    T: FromBytes + CLTyped,
{
    let key = match provider.named_keys_get(name) {
        None => {
            error!("auction missing named key {:?}", name);
            return Err(Error::MissingKey);
        }
        Some(key) => key,
    };
    let uref = key.into_uref().ok_or(Error::InvalidKeyVariant)?;
    let value: T = provider.read(uref)?.ok_or(Error::MissingValue)?;
    Ok(value)
}

fn write_to<P, T>(provider: &mut P, name: &str, value: T) -> Result<(), Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
    T: ToBytes + CLTyped,
{
    let key = provider.named_keys_get(name).ok_or(Error::MissingKey)?;
    let uref = key.into_uref().ok_or(Error::InvalidKeyVariant)?;
    provider.write(uref, value)
}

/// Aggregated bid data for a Validator.
#[derive(Debug, Default)]
pub struct ValidatorBidsDetail {
    validator_bids: ValidatorBids,
    validator_credits: ValidatorCredits,
    delegator_bids: DelegatorBids,
    reservations: Reservations,
}

impl ValidatorBidsDetail {
    /// Ctor.
    pub fn new() -> Self {
        ValidatorBidsDetail {
            validator_bids: BTreeMap::new(),
            validator_credits: BTreeMap::new(),
            delegator_bids: BTreeMap::new(),
            reservations: BTreeMap::new(),
        }
    }

    /// Inserts a validator bid.
    pub fn insert_bid(
        &mut self,
        validator: PublicKey,
        validator_bid: Box<ValidatorBid>,
        delegators: Vec<Box<DelegatorBid>>,
        reservations: Vec<Box<Reservation>>,
    ) -> Option<Box<ValidatorBid>> {
        self.delegator_bids.insert(validator.clone(), delegators);
        self.reservations.insert(validator.clone(), reservations);
        self.validator_bids.insert(validator, validator_bid)
    }

    /// Inserts a validator credit.
    pub fn insert_credit(
        &mut self,
        validator: PublicKey,
        era_id: EraId,
        validator_credit: Box<ValidatorCredit>,
    ) {
        let credits = &mut self.validator_credits;

        credits
            .entry(validator.clone())
            .and_modify(|inner| {
                inner
                    .entry(era_id)
                    .and_modify(|_| {
                        warn!(
                            ?validator,
                            ?era_id,
                            "multiple validator credit entries in same era"
                        )
                    })
                    .or_insert(validator_credit.clone());
            })
            .or_insert_with(|| {
                let mut inner = BTreeMap::new();
                inner.insert(era_id, validator_credit);
                inner
            });
    }

    /// Get validator weights.
    #[allow(clippy::too_many_arguments)]
    pub fn validator_weights(
        &mut self,
        era_ending: EraId,
        era_end_timestamp_millis: u64,
        vesting_schedule_period_millis: u64,
        locked: bool,
        include_credits: bool,
        cap: Ratio<U512>,
    ) -> Result<ValidatorWeights, Error> {
        let mut ret = BTreeMap::new();

        for (validator_public_key, bid) in self.validator_bids.iter().filter(|(_, v)| {
            locked
                == v.is_locked_with_vesting_schedule(
                    era_end_timestamp_millis,
                    vesting_schedule_period_millis,
                )
                && !v.inactive()
        }) {
            let mut staked_amount = bid.staked_amount();
            if let Some(delegators) = self.delegator_bids.get(validator_public_key) {
                staked_amount = staked_amount
                    .checked_add(delegators.iter().map(|d| d.staked_amount()).sum())
                    .ok_or(Error::InvalidAmount)?;
            }

            let credit_amount = self.credit_amount(
                validator_public_key,
                era_ending,
                staked_amount,
                include_credits,
                cap,
            );
            let total = staked_amount.saturating_add(credit_amount);
            ret.insert(validator_public_key.clone(), total);
        }

        Ok(ret)
    }

    fn credit_amount(
        &self,
        validator_public_key: &PublicKey,
        era_ending: EraId,
        staked_amount: U512,
        include_credit: bool,
        cap: Ratio<U512>,
    ) -> U512 {
        if !include_credit {
            return U512::zero();
        }

        if let Some(inner) = self.validator_credits.get(validator_public_key) {
            if let Some(credit) = inner.get(&era_ending) {
                let capped = Ratio::new_raw(staked_amount, U512::one())
                    .mul(cap)
                    .to_integer();
                let credit_amount = credit.amount();
                return credit_amount.min(capped);
            }
        }

        U512::zero()
    }

    pub(crate) fn validator_bids(&self) -> &ValidatorBids {
        &self.validator_bids
    }

    pub(crate) fn validator_bids_mut(&mut self) -> &mut ValidatorBids {
        &mut self.validator_bids
    }

    /// Consume self into in underlying collections.
    pub fn destructure(self) -> (ValidatorBids, ValidatorCredits, DelegatorBids, Reservations) {
        (
            self.validator_bids,
            self.validator_credits,
            self.delegator_bids,
            self.reservations,
        )
    }
}

/// Prunes away all validator credits for the imputed era, which should be the era ending.
///
/// This is intended to be called at the end of an era, after calculating validator weights.
pub fn prune_validator_credits<P>(
    provider: &mut P,
    era_ending: EraId,
    validator_credits: &ValidatorCredits,
) where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    for (validator_public_key, inner) in validator_credits {
        if inner.contains_key(&era_ending) {
            provider.prune_bid(BidAddr::new_credit(validator_public_key, era_ending))
        }
    }
}

/// Returns the imputed validator bids.
pub fn get_validator_bids<P>(provider: &mut P, era_id: EraId) -> Result<ValidatorBidsDetail, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    let bids_keys = provider.get_keys(&KeyTag::BidAddr)?;

    let mut ret = ValidatorBidsDetail::new();

    for key in bids_keys {
        match provider.read_bid(&key)? {
            Some(BidKind::Validator(validator_bid)) => {
                let validator_public_key = validator_bid.validator_public_key();
                let delegator_bids = delegators(provider, validator_public_key)?;
                let reservations = reservations(provider, validator_public_key)?;
                ret.insert_bid(
                    validator_public_key.clone(),
                    validator_bid,
                    delegator_bids,
                    reservations,
                );
            }
            Some(BidKind::Credit(credit)) => {
                ret.insert_credit(credit.validator_public_key().clone(), era_id, credit);
            }
            Some(_) => {
                // noop
            }
            None => return Err(Error::ValidatorNotFound),
        };
    }

    Ok(ret)
}

/// Sets the imputed validator bids.
pub fn set_validator_bids<P>(provider: &mut P, validators: ValidatorBids) -> Result<(), Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    for (validator_public_key, validator_bid) in validators.into_iter() {
        let bid_addr = BidAddr::from(validator_public_key.clone());
        provider.write_bid(bid_addr.into(), BidKind::Validator(validator_bid))?;
    }
    Ok(())
}

/// Returns the unbonding purses.
pub fn get_unbonding_purses<P>(provider: &mut P) -> Result<BTreeMap<BidAddr, Unbond>, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    let prefix = vec![KeyTag::BidAddr as u8, BidAddrTag::UnbondAccount as u8];

    let unbond_keys = provider.get_keys_by_prefix(&prefix)?;

    let mut ret = BTreeMap::new();

    for key in unbond_keys {
        if let Key::BidAddr(bid_addr) = key {
            match provider.read_bid(&key) {
                Ok(Some(BidKind::Unbond(unbonds))) => {
                    ret.insert(bid_addr, *unbonds);
                }
                Ok(Some(_)) => {
                    warn!("unexpected BidKind variant {:?}", key);
                }
                Ok(None) => {
                    warn!("expected unbond record {:?}", key);
                }
                Err(err) => {
                    error!("{} {}", key, err);
                }
            }
        }
    }

    let prefix = vec![KeyTag::BidAddr as u8, BidAddrTag::UnbondPurse as u8];

    let unbond_keys = provider.get_keys_by_prefix(&prefix)?;
    for key in unbond_keys {
        if let Key::BidAddr(bid_addr) = key {
            match provider.read_bid(&key) {
                Ok(Some(BidKind::Unbond(unbonds))) => {
                    ret.insert(bid_addr, *unbonds);
                }
                Ok(Some(_)) => {
                    warn!("unexpected BidKind variant {:?}", key)
                }
                Ok(None) => {
                    warn!("expected unbond record {:?}", key)
                }
                Err(err) => {
                    error!("{} {}", key, err);
                }
            }
        }
    }

    Ok(ret)
}

/// Returns the era id.
pub fn get_era_id<P>(provider: &mut P) -> Result<EraId, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    read_from(provider, ERA_ID_KEY)
}

/// Sets the era id.
pub fn set_era_id<P>(provider: &mut P, era_id: EraId) -> Result<(), Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    write_to(provider, ERA_ID_KEY, era_id)
}

/// Returns the era end timestamp.
pub fn get_era_end_timestamp_millis<P>(provider: &mut P) -> Result<u64, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    read_from(provider, ERA_END_TIMESTAMP_MILLIS_KEY)
}

/// Sets the era end timestamp.
pub fn set_era_end_timestamp_millis<P>(
    provider: &mut P,
    era_end_timestamp_millis: u64,
) -> Result<(), Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    write_to(
        provider,
        ERA_END_TIMESTAMP_MILLIS_KEY,
        era_end_timestamp_millis,
    )
}

/// Returns seigniorage recipients snapshot.
pub fn get_seigniorage_recipients_snapshot<P>(
    provider: &mut P,
) -> Result<SeigniorageRecipientsSnapshotV2, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    read_from(provider, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)
}

/// Returns seigniorage recipients snapshot in legacy format.
pub fn get_legacy_seigniorage_recipients_snapshot<P>(
    provider: &mut P,
) -> Result<SeigniorageRecipientsSnapshotV1, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    read_from(provider, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)
}

/// Sets the setigniorage recipients snapshot.
pub fn set_seigniorage_recipients_snapshot<P>(
    provider: &mut P,
    snapshot: SeigniorageRecipientsSnapshotV2,
) -> Result<(), Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    write_to(provider, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, snapshot)
}

/// Returns the number of validator slots.
pub fn get_validator_slots<P>(provider: &mut P) -> Result<usize, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    let validator_slots: u32 = match read_from(provider, VALIDATOR_SLOTS_KEY) {
        Ok(ret) => ret,
        Err(err) => {
            error!("Failed to find VALIDATOR_SLOTS_KEY {}", err);
            return Err(err);
        }
    };
    let validator_slots = validator_slots
        .try_into()
        .map_err(|_| Error::InvalidValidatorSlotsValue)?;
    Ok(validator_slots)
}

/// Returns auction delay.
pub fn get_auction_delay<P>(provider: &mut P) -> Result<u64, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    let auction_delay: u64 = match read_from(provider, AUCTION_DELAY_KEY) {
        Ok(ret) => ret,
        Err(err) => {
            error!("Failed to find AUCTION_DELAY_KEY {}", err);
            return Err(err);
        }
    };
    Ok(auction_delay)
}

fn get_unbonding_delay<P>(provider: &mut P) -> Result<u64, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    read_from(provider, UNBONDING_DELAY_KEY)
}

/// Iterates over unbonding entries and checks if a locked amount can be paid already if
/// a specific era is reached.
///
/// This function can be called by the system only.
pub fn process_unbond_requests<P: Auction>(
    provider: &mut P,
    max_delegators_per_validator: u32,
) -> Result<(), ApiError> {
    if provider.get_caller() != PublicKey::System.to_account_hash() {
        return Err(Error::InvalidCaller.into());
    }

    let current_era_id = provider.read_era_id()?;

    let unbonding_delay = get_unbonding_delay(provider)?;

    let unbonds = get_unbonding_purses(provider)?;

    for (bid_addr, unbond) in unbonds {
        let unbond_kind = &unbond.unbond_kind().clone();
        let (retained, expired) = unbond.expired(current_era_id, unbonding_delay);
        if let Some(unbonded) = expired {
            for unbond_era in unbonded {
                if unbond_kind.is_validator() {
                    provider.unbond(unbond_kind, &unbond_era).map_err(|err| {
                        error!(?err, "error unbonding purse");
                        ApiError::from(Error::TransferToUnbondingPurse)
                    })?;
                    continue;
                }
                let redelegation_result = handle_redelegation(
                    provider,
                    unbond_kind,
                    &unbond_era,
                    max_delegators_per_validator,
                )
                .inspect_err(|err| {
                    error!(?err, ?unbond_kind, ?unbond_era, "error processing unbond");
                })?;

                match redelegation_result {
                    UnbondRedelegationOutcome::SuccessfullyRedelegated => {
                        // noop; on successful re-delegation, no actual unbond occurs
                    }
                    uro @ UnbondRedelegationOutcome::NonexistantRedelegationTarget
                    | uro @ UnbondRedelegationOutcome::DelegationAmountBelowCap
                    | uro @ UnbondRedelegationOutcome::DelegationAmountAboveCap
                    | uro @ UnbondRedelegationOutcome::RedelegationTargetHasNoVacancy
                    | uro @ UnbondRedelegationOutcome::RedelegationTargetIsUnstaked
                    | uro @ UnbondRedelegationOutcome::Withdrawal => {
                        // Move funds from bid purse to unbonding purse
                        provider.unbond(unbond_kind, &unbond_era).map_err(|err| {
                            error!(?err, ?uro, "error unbonding purse");
                            ApiError::from(Error::TransferToUnbondingPurse)
                        })?
                    }
                }
            }
        }
        if retained.eras().is_empty() {
            provider.write_unbond(bid_addr, None)?;
        } else {
            provider.write_unbond(bid_addr, Some(retained))?;
        }
    }
    Ok(())
}

/// Creates a new purse in unbonding_purses given a validator's key, amount, and a destination
/// unbonding purse. Returns the amount of motes remaining in the validator's bid purse.
pub fn create_unbonding_purse<P: Auction>(
    provider: &mut P,
    validator_public_key: PublicKey,
    unbond_kind: UnbondKind,
    bonding_purse: URef,
    amount: U512,
    new_validator: Option<PublicKey>,
) -> Result<(), Error> {
    if provider
        .available_balance(bonding_purse)?
        .unwrap_or_default()
        < amount
    {
        return Err(Error::UnbondTooLarge);
    }

    let era_of_creation = provider.read_era_id()?;

    let bid_addr = match &unbond_kind {
        UnbondKind::Validator(_) => {
            let account_hash = validator_public_key.to_account_hash();
            BidAddr::UnbondAccount {
                validator: account_hash,
                unbonder: account_hash,
            }
        }
        UnbondKind::DelegatedPublicKey(pk) => BidAddr::UnbondAccount {
            validator: validator_public_key.to_account_hash(),
            unbonder: pk.to_account_hash(),
        },
        UnbondKind::DelegatedPurse(addr) => BidAddr::UnbondPurse {
            validator: validator_public_key.to_account_hash(),
            unbonder: *addr,
        },
    };

    let unbond_era = UnbondEra::new(bonding_purse, era_of_creation, amount, new_validator);

    let unbond = match provider.read_unbond(bid_addr)? {
        Some(unbond) => {
            let mut eras = unbond.take_eras();
            eras.push(unbond_era);
            Unbond::new(validator_public_key, unbond_kind, eras)
        }
        None => Unbond::new(validator_public_key, unbond_kind, vec![unbond_era]),
    };

    provider.write_unbond(bid_addr, Some(unbond))?;

    Ok(())
}

/// Returns most recent validator public key if public key has been changed
/// or the validator has withdrawn their bid completely.
pub fn get_most_recent_validator_public_key<P>(
    provider: &mut P,
    mut validator_public_key: PublicKey,
) -> Result<PublicKey, Error>
where
    P: RuntimeProvider + StorageProvider,
{
    let mut validator_bid_addr = BidAddr::from(validator_public_key.clone());
    let mut found_validator_bid_chain_tip = false;
    for _ in 0..MAX_BRIDGE_CHAIN_LENGTH {
        match provider.read_bid(&validator_bid_addr.into())? {
            Some(BidKind::Validator(validator_bid)) => {
                validator_public_key = validator_bid.validator_public_key().clone();
                found_validator_bid_chain_tip = true;
                break;
            }
            Some(BidKind::Bridge(bridge)) => {
                validator_public_key = bridge.new_validator_public_key().clone();
                validator_bid_addr = BidAddr::from(validator_public_key.clone());
            }
            _ => {
                // Validator has withdrawn their bid, so there's nothing at the tip.
                // In this case we add the reward to a delegator's unbond.
                found_validator_bid_chain_tip = true;
                break;
            }
        };
    }
    if !found_validator_bid_chain_tip {
        Err(Error::BridgeRecordChainTooLong)
    } else {
        Ok(validator_public_key)
    }
}

/// Attempts to apply the delegator reward to the existing stake. If the reward recipient has
/// completely unstaked, applies it to their unbond instead. In either case, returns
/// the purse the amount should be applied to.
pub fn distribute_delegator_rewards<P>(
    provider: &mut P,
    seigniorage_allocations: &mut Vec<SeigniorageAllocation>,
    validator_public_key: PublicKey,
    rewards: impl IntoIterator<Item = (DelegatorKind, U512)>,
) -> Result<Vec<(DelegatorKind, U512, URef)>, Error>
where
    P: RuntimeProvider + StorageProvider,
{
    let mut delegator_payouts = Vec::new();
    for (delegator_kind, delegator_reward_trunc) in rewards {
        let bid_addr = BidAddr::new_delegator_kind(&validator_public_key, &delegator_kind);
        let bid_key = bid_addr.into();

        let delegator_bonding_purse = match read_delegator_bid(provider, &bid_key) {
            Ok(mut delegator_bid) if !delegator_bid.staked_amount().is_zero() => {
                let purse = *delegator_bid.bonding_purse();
                delegator_bid.increase_stake(delegator_reward_trunc)?;
                provider.write_bid(bid_key, BidKind::Delegator(delegator_bid))?;
                purse
            }
            Ok(_) | Err(Error::DelegatorNotFound) => {
                // check to see if there are unbond entries for this recipient
                // (validator + delegator match), and if there are apply the amount
                // to the unbond entry with the highest era.

                let unbond_bid_addr = match &delegator_kind {
                    DelegatorKind::PublicKey(pk) => BidAddr::UnbondAccount {
                        validator: validator_public_key.to_account_hash(),
                        unbonder: pk.to_account_hash(),
                    },
                    DelegatorKind::Purse(addr) => BidAddr::UnbondPurse {
                        validator: validator_public_key.to_account_hash(),
                        unbonder: *addr,
                    },
                };

                match provider.read_unbond(unbond_bid_addr)?.as_mut() {
                    Some(unbond) => {
                        match unbond
                            .eras_mut()
                            .iter_mut()
                            .max_by(|x, y| x.era_of_creation().cmp(&y.era_of_creation()))
                        {
                            Some(unbond_era) => {
                                let purse = *unbond_era.bonding_purse();
                                let new_amount =
                                    unbond_era.amount().saturating_add(delegator_reward_trunc);
                                unbond_era.with_amount(new_amount);
                                provider.write_unbond(unbond_bid_addr, Some(unbond.clone()))?;
                                purse
                            }
                            None => return Err(Error::DelegatorNotFound),
                        }
                    }
                    None => return Err(Error::DelegatorNotFound),
                }
            }
            Err(err) => {
                return Err(err);
            }
        };

        delegator_payouts.push((
            delegator_kind.clone(),
            delegator_reward_trunc,
            delegator_bonding_purse,
        ));

        let allocation = SeigniorageAllocation::delegator_kind(
            delegator_kind,
            validator_public_key.clone(),
            delegator_reward_trunc,
        );

        seigniorage_allocations.push(allocation);
    }

    Ok(delegator_payouts)
}

/// Attempts to apply the validator reward to the existing stake. If the reward recipient has
/// completely unstaked, applies it to their unbond instead. In either case, returns
/// the purse the amount should be applied to.
pub fn distribute_validator_rewards<P>(
    provider: &mut P,
    seigniorage_allocations: &mut Vec<SeigniorageAllocation>,
    validator_public_key: PublicKey,
    amount: U512,
) -> Result<URef, Error>
where
    P: StorageProvider,
{
    let bid_key: Key = BidAddr::from(validator_public_key.clone()).into();
    let bonding_purse = match read_current_validator_bid(provider, bid_key) {
        Ok(mut validator_bid) => {
            let purse = *validator_bid.bonding_purse();
            validator_bid.increase_stake(amount)?;
            provider.write_bid(bid_key, BidKind::Validator(validator_bid))?;
            purse
        }
        Err(Error::ValidatorNotFound) => {
            // check to see if there are unbond entries for this recipient, and if there are
            // apply the amount to the unbond entry with the highest era.
            let account_hash = validator_public_key.to_account_hash();
            let unbond_addr = BidAddr::UnbondAccount {
                validator: account_hash,
                unbonder: account_hash,
            };
            match provider.read_unbond(unbond_addr)?.as_mut() {
                Some(unbond) => {
                    match unbond
                        .eras_mut()
                        .iter_mut()
                        .max_by(|x, y| x.era_of_creation().cmp(&y.era_of_creation()))
                    {
                        Some(unbond_era) => {
                            let purse = *unbond_era.bonding_purse();
                            let new_amount = unbond_era.amount().saturating_add(amount);
                            unbond_era.with_amount(new_amount);
                            provider.write_unbond(unbond_addr, Some(unbond.clone()))?;
                            purse
                        }
                        None => return Err(Error::ValidatorNotFound),
                    }
                }
                None => return Err(Error::ValidatorNotFound),
            }
        }
        Err(err) => return Err(err),
    };

    let allocation = SeigniorageAllocation::validator(validator_public_key, amount);
    seigniorage_allocations.push(allocation);
    Ok(bonding_purse)
}

#[derive(Debug)]
enum UnbondRedelegationOutcome {
    Withdrawal,
    SuccessfullyRedelegated,
    NonexistantRedelegationTarget,
    RedelegationTargetHasNoVacancy,
    RedelegationTargetIsUnstaked,
    DelegationAmountBelowCap,
    DelegationAmountAboveCap,
}

fn handle_redelegation<P>(
    provider: &mut P,
    unbond_kind: &UnbondKind,
    unbond_era: &UnbondEra,
    max_delegators_per_validator: u32,
) -> Result<UnbondRedelegationOutcome, ApiError>
where
    P: StorageProvider + MintProvider + RuntimeProvider,
{
    let delegator_kind = match unbond_kind {
        UnbondKind::Validator(_) => {
            return Err(ApiError::AuctionError(Error::UnexpectedUnbondVariant as u8))
        }
        UnbondKind::DelegatedPublicKey(pk) => DelegatorKind::PublicKey(pk.clone()),
        UnbondKind::DelegatedPurse(addr) => DelegatorKind::Purse(*addr),
    };

    let redelegation_target_public_key = match unbond_era.new_validator() {
        Some(public_key) => {
            // get updated key if `ValidatorBid` public key was changed
            let validator_bid_addr = BidAddr::from(public_key.clone());
            match read_current_validator_bid(provider, validator_bid_addr.into()) {
                Ok(validator_bid) => validator_bid.validator_public_key().clone(),
                Err(err) => {
                    error!(?err, ?unbond_era, redelegate_to=?public_key, "error redelegating");
                    return Ok(UnbondRedelegationOutcome::NonexistantRedelegationTarget);
                }
            }
        }
        None => return Ok(UnbondRedelegationOutcome::Withdrawal),
    };

    let redelegation = handle_delegation(
        provider,
        delegator_kind,
        redelegation_target_public_key,
        *unbond_era.bonding_purse(),
        *unbond_era.amount(),
        max_delegators_per_validator,
    );
    match redelegation {
        Ok(_) => Ok(UnbondRedelegationOutcome::SuccessfullyRedelegated),
        Err(ApiError::AuctionError(err)) if err == Error::BondTooSmall as u8 => {
            Ok(UnbondRedelegationOutcome::RedelegationTargetIsUnstaked)
        }
        Err(ApiError::AuctionError(err)) if err == Error::DelegationAmountTooSmall as u8 => {
            Ok(UnbondRedelegationOutcome::DelegationAmountBelowCap)
        }
        Err(ApiError::AuctionError(err)) if err == Error::DelegationAmountTooLarge as u8 => {
            Ok(UnbondRedelegationOutcome::DelegationAmountAboveCap)
        }
        Err(ApiError::AuctionError(err)) if err == Error::ValidatorNotFound as u8 => {
            Ok(UnbondRedelegationOutcome::NonexistantRedelegationTarget)
        }
        Err(ApiError::AuctionError(err)) if err == Error::ExceededDelegatorSizeLimit as u8 => {
            Ok(UnbondRedelegationOutcome::RedelegationTargetHasNoVacancy)
        }
        Err(err) => Err(err),
    }
}

/// Checks if a reservation for a given delegator exists.
fn has_reservation<P>(
    provider: &mut P,
    delegator_kind: &DelegatorKind,
    validator: &PublicKey,
) -> Result<bool, Error>
where
    P: RuntimeProvider + StorageProvider + ?Sized,
{
    let reservation_bid_key = match delegator_kind {
        DelegatorKind::PublicKey(pk) => BidAddr::ReservedDelegationAccount {
            validator: validator.to_account_hash(),
            delegator: pk.to_account_hash(),
        },
        DelegatorKind::Purse(addr) => BidAddr::ReservedDelegationPurse {
            validator: validator.to_account_hash(),
            delegator: *addr,
        },
    }
    .into();
    if let Some(BidKind::Reservation(_)) = provider.read_bid(&reservation_bid_key)? {
        Ok(true)
    } else {
        Ok(false)
    }
}

/// If specified validator exists, and if validator is not yet at max delegators count, processes
/// delegation. For a new delegation a delegator bid record will be created to track the delegation,
/// otherwise the existing tracking record will be updated.
#[allow(clippy::too_many_arguments)]
pub fn handle_delegation<P>(
    provider: &mut P,
    delegator_kind: DelegatorKind,
    validator_public_key: PublicKey,
    source: URef,
    amount: U512,
    max_delegators_per_validator: u32,
) -> Result<U512, ApiError>
where
    P: StorageProvider + MintProvider + RuntimeProvider,
{
    if amount.is_zero() {
        return Err(Error::BondTooSmall.into());
    }

    let validator_bid_addr = BidAddr::from(validator_public_key.clone());
    // is there such a validator?
    let validator_bid = read_validator_bid(provider, &validator_bid_addr.into())?;
    if amount < U512::from(validator_bid.minimum_delegation_amount()) {
        return Err(Error::DelegationAmountTooSmall.into());
    }
    if amount > U512::from(validator_bid.maximum_delegation_amount()) {
        return Err(Error::DelegationAmountTooLarge.into());
    }

    // is there already a record for this delegator?
    let delegator_bid_key =
        BidAddr::new_delegator_kind(&validator_public_key, &delegator_kind).into();

    let (target, delegator_bid) = if let Some(BidKind::Delegator(mut delegator_bid)) =
        provider.read_bid(&delegator_bid_key)?
    {
        delegator_bid.increase_stake(amount)?;
        (*delegator_bid.bonding_purse(), delegator_bid)
    } else {
        // is this validator over the delegator limit
        // or is there a reservation for given delegator public key?
        let delegator_count = provider.delegator_count(&validator_bid_addr)?;
        let reserved_slots_count = validator_bid.reserved_slots();
        let reservation_count = provider.reservation_count(&validator_bid_addr)?;
        let has_reservation = has_reservation(provider, &delegator_kind, &validator_public_key)?;
        if delegator_count >= (max_delegators_per_validator - reserved_slots_count) as usize
            && !has_reservation
        {
            warn!(
                %delegator_count, %max_delegators_per_validator, %reservation_count, %has_reservation,
                "delegator_count {}, max_delegators_per_validator {}, reservation_count {}, has_reservation {}",
                delegator_count, max_delegators_per_validator, reservation_count, has_reservation
            );
            return Err(Error::ExceededDelegatorSizeLimit.into());
        }

        let bonding_purse = provider.create_purse()?;
        let delegator_bid =
            DelegatorBid::unlocked(delegator_kind, amount, bonding_purse, validator_public_key);
        (bonding_purse, Box::new(delegator_bid))
    };

    // transfer token to bonding purse
    provider
        .mint_transfer_direct(
            Some(PublicKey::System.to_account_hash()),
            source,
            target,
            amount,
            None,
        )
        .map_err(|_| Error::TransferToDelegatorPurse)?
        .map_err(|mint_error| {
            // Propagate mint contract's error that occured during execution of transfer
            // entrypoint. This will improve UX in case of (for example)
            // unapproved spending limit error.
            ApiError::from(mint_error)
        })?;

    let updated_amount = delegator_bid.staked_amount();
    provider.write_bid(delegator_bid_key, BidKind::Delegator(delegator_bid))?;

    Ok(updated_amount)
}

/// If specified validator exists, and if validator is not yet at max reservations count, processes
/// reservation. For a new reservation a bid record will be created to track the reservation,
/// otherwise the existing tracking record will be updated.
#[allow(clippy::too_many_arguments)]
pub fn handle_add_reservation<P>(provider: &mut P, reservation: Reservation) -> Result<(), Error>
where
    P: StorageProvider + MintProvider + RuntimeProvider,
{
    // is there such a validator?
    let validator_bid_addr = BidAddr::from(reservation.validator_public_key().clone());
    let bid = read_validator_bid(provider, &validator_bid_addr.into())?;

    let reservation_bid_key = match reservation.delegator_kind() {
        DelegatorKind::PublicKey(pk) => BidAddr::ReservedDelegationAccount {
            validator: reservation.validator_public_key().to_account_hash(),
            delegator: pk.to_account_hash(),
        },
        DelegatorKind::Purse(addr) => BidAddr::ReservedDelegationPurse {
            validator: reservation.validator_public_key().to_account_hash(),
            delegator: *addr,
        },
    }
    .into();
    if provider.read_bid(&reservation_bid_key)?.is_none() {
        // ensure reservation list has capacity to create a new reservation
        let reservation_count = provider.reservation_count(&validator_bid_addr)?;
        let reserved_slots = bid.reserved_slots() as usize;
        if reservation_count >= reserved_slots {
            warn!(
                %reservation_count, %reserved_slots,
                "reservation_count {}, reserved_slots {}",
                reservation_count, reserved_slots
            );
            return Err(Error::ExceededReservationsLimit);
        }
    };

    // validate specified delegation rate
    if reservation.delegation_rate() > &DELEGATION_RATE_DENOMINATOR {
        return Err(Error::DelegationRateTooLarge);
    }

    provider.write_bid(
        reservation_bid_key,
        BidKind::Reservation(Box::new(reservation)),
    )?;

    Ok(())
}

/// Attempts to remove a reservation if one exists. If not it returns an error.
///
/// If there is already a delegator bid associated with a given reservation it validates that
/// there are free public slots available. If not, it returns an error since the delegator
/// cannot be "downgraded".
pub fn handle_cancel_reservation<P>(
    provider: &mut P,
    validator: PublicKey,
    delegator_kind: DelegatorKind,
    max_delegators_per_validator: u32,
) -> Result<(), Error>
where
    P: StorageProvider + MintProvider + RuntimeProvider,
{
    // is there such a validator?
    let validator_bid_addr = BidAddr::from(validator.clone());
    let validator_bid = read_validator_bid(provider, &validator_bid_addr.into())?;
    let validator = validator.to_account_hash();

    // is there a reservation for this delegator?
    let (reservation_bid_addr, delegator_bid_addr) = match delegator_kind {
        DelegatorKind::PublicKey(pk) => {
            let delegator_account_hash = pk.to_account_hash();
            (
                BidAddr::ReservedDelegationAccount {
                    validator,
                    delegator: delegator_account_hash,
                },
                BidAddr::DelegatedAccount {
                    validator,
                    delegator: delegator_account_hash,
                },
            )
        }
        DelegatorKind::Purse(addr) => (
            BidAddr::ReservedDelegationPurse {
                validator,
                delegator: addr,
            },
            BidAddr::DelegatedPurse {
                validator,
                delegator: addr,
            },
        ),
    };

    if provider.read_bid(&reservation_bid_addr.into())?.is_none() {
        return Err(Error::ReservationNotFound);
    }

    // is there such a delegator?
    if read_delegator_bid(provider, &delegator_bid_addr.into()).is_ok() {
        // is there a free public slot
        let reserved_slots = validator_bid.reserved_slots();
        let delegator_count = provider.delegator_count(&validator_bid_addr)?;
        let used_reservation_count = provider.used_reservation_count(&validator_bid_addr)?;
        let normal_delegators = delegator_count.saturating_sub(used_reservation_count);
        let public_slots = max_delegators_per_validator.saturating_sub(reserved_slots);

        // cannot "downgrade" a delegator if there are no free public slots available
        if public_slots == normal_delegators as u32 {
            return Err(Error::ExceededDelegatorSizeLimit);
        }
    }

    provider.prune_bid(reservation_bid_addr);
    Ok(())
}

/// Returns validator bid by key.
pub fn read_validator_bid<P>(provider: &mut P, bid_key: &Key) -> Result<Box<ValidatorBid>, Error>
where
    P: StorageProvider + ?Sized,
{
    if !bid_key.is_bid_addr_key() {
        return Err(Error::InvalidKeyVariant);
    }
    if let Some(BidKind::Validator(validator_bid)) = provider.read_bid(bid_key)? {
        Ok(validator_bid)
    } else {
        Err(Error::ValidatorNotFound)
    }
}

/// Returns current `ValidatorBid` in case the public key was changed.
pub fn read_current_validator_bid<P>(
    provider: &mut P,
    mut bid_key: Key,
) -> Result<Box<ValidatorBid>, Error>
where
    P: StorageProvider + ?Sized,
{
    if !bid_key.is_bid_addr_key() {
        return Err(Error::InvalidKeyVariant);
    }

    for _ in 0..MAX_BRIDGE_CHAIN_LENGTH {
        match provider.read_bid(&bid_key)? {
            Some(BidKind::Validator(validator_bid)) => return Ok(validator_bid),
            Some(BidKind::Bridge(bridge)) => {
                debug!(
                    ?bid_key,
                    ?bridge,
                    "read_current_validator_bid: bridge found"
                );
                let validator_bid_addr = BidAddr::from(bridge.new_validator_public_key().clone());
                bid_key = validator_bid_addr.into();
            }
            _ => break,
        }
    }
    Err(Error::ValidatorNotFound)
}

/// Returns all delegator bids for imputed validator.
pub fn read_delegator_bids<P>(
    provider: &mut P,
    validator_public_key: &PublicKey,
) -> Result<Vec<DelegatorBid>, Error>
where
    P: RuntimeProvider + StorageProvider + ?Sized,
{
    let mut ret = vec![];
    let bid_addr = BidAddr::from(validator_public_key.clone());
    let mut delegator_bid_keys = provider.get_keys_by_prefix(
        &bid_addr
            .delegated_account_prefix()
            .map_err(|_| Error::Serialization)?,
    )?;
    delegator_bid_keys.extend(
        provider.get_keys_by_prefix(
            &bid_addr
                .delegated_purse_prefix()
                .map_err(|_| Error::Serialization)?,
        )?,
    );
    for delegator_bid_key in delegator_bid_keys {
        let delegator_bid = read_delegator_bid(provider, &delegator_bid_key)?;
        ret.push(*delegator_bid);
    }

    Ok(ret)
}

/// Returns delegator bid by key.
pub fn read_delegator_bid<P>(provider: &mut P, bid_key: &Key) -> Result<Box<DelegatorBid>, Error>
where
    P: RuntimeProvider + ?Sized + StorageProvider,
{
    if !bid_key.is_bid_addr_key() {
        return Err(Error::InvalidKeyVariant);
    }
    if let Some(BidKind::Delegator(delegator_bid)) = provider.read_bid(bid_key)? {
        Ok(delegator_bid)
    } else {
        Err(Error::DelegatorNotFound)
    }
}

/// Returns all delegator slot reservations for given validator.
pub fn read_reservation_bids<P>(
    provider: &mut P,
    validator_public_key: &PublicKey,
) -> Result<Vec<Reservation>, Error>
where
    P: RuntimeProvider + StorageProvider + ?Sized,
{
    let mut ret = vec![];
    let bid_addr = BidAddr::from(validator_public_key.clone());
    let mut reservation_bid_keys = provider.get_keys_by_prefix(
        &bid_addr
            .reserved_account_prefix()
            .map_err(|_| Error::Serialization)?,
    )?;
    reservation_bid_keys.extend(
        provider.get_keys_by_prefix(
            &bid_addr
                .reserved_purse_prefix()
                .map_err(|_| Error::Serialization)?,
        )?,
    );
    for reservation_bid_key in reservation_bid_keys {
        let reservation_bid = read_reservation_bid(provider, &reservation_bid_key)?;
        ret.push(*reservation_bid);
    }

    Ok(ret)
}

/// Returns delegator slot reservation bid by key.
pub fn read_reservation_bid<P>(provider: &mut P, bid_key: &Key) -> Result<Box<Reservation>, Error>
where
    P: RuntimeProvider + ?Sized + StorageProvider,
{
    if !bid_key.is_bid_addr_key() {
        return Err(Error::InvalidKeyVariant);
    }
    if let Some(BidKind::Reservation(reservation_bid)) = provider.read_bid(bid_key)? {
        Ok(reservation_bid)
    } else {
        Err(Error::ReservationNotFound)
    }
}

/// Applies seigniorage recipient changes.
pub fn seigniorage_recipients(
    validator_weights: &ValidatorWeights,
    validator_bids: &ValidatorBids,
    delegator_bids: &DelegatorBids,
    reservations: &Reservations,
) -> Result<SeigniorageRecipientsV2, Error> {
    let mut recipients = SeigniorageRecipientsV2::new();
    for (validator_public_key, validator_total_weight) in validator_weights {
        // check if validator bid exists before processing.
        let validator_bid = validator_bids
            .get(validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;
        // calculate delegator portion(s), if any
        let mut delegators_weight = U512::zero();
        let mut delegators_stake = BTreeMap::new();
        if let Some(delegators) = delegator_bids.get(validator_public_key) {
            for delegator_bid in delegators {
                if delegator_bid.staked_amount().is_zero() {
                    continue;
                }
                let delegator_staked_amount = delegator_bid.staked_amount();
                delegators_weight = delegators_weight.saturating_add(delegator_staked_amount);
                let delegator_kind = delegator_bid.delegator_kind();
                delegators_stake.insert(delegator_kind.clone(), delegator_staked_amount);
            }
        }

        let mut reservation_delegation_rates = BTreeMap::new();
        if let Some(reservations) = reservations.get(validator_public_key) {
            for reservation in reservations {
                reservation_delegation_rates.insert(
                    reservation.delegator_kind().clone(),
                    *reservation.delegation_rate(),
                );
            }
        }

        // determine validator's personal stake (total weight - sum of delegators weight)
        let validator_stake = validator_total_weight.saturating_sub(delegators_weight);
        let seigniorage_recipient = SeigniorageRecipientV2::new(
            validator_stake,
            *validator_bid.delegation_rate(),
            delegators_stake,
            reservation_delegation_rates,
        );
        recipients.insert(validator_public_key.clone(), seigniorage_recipient);
    }
    Ok(recipients)
}

/// Returns the era validators from a snapshot.
///
/// This is `pub` as it is used not just in the relevant auction entry point, but also by the
/// engine state while directly querying for the era validators.
pub fn era_validators_from_snapshot(snapshot: SeigniorageRecipientsSnapshotV2) -> EraValidators {
    snapshot
        .into_iter()
        .map(|(era_id, recipients)| {
            let validator_weights = recipients
                .into_iter()
                .filter_map(|(public_key, bid)| bid.total_stake().map(|stake| (public_key, stake)))
                .collect::<ValidatorWeights>();
            (era_id, validator_weights)
        })
        .collect()
}

/// Returns the era validators from a legacy snapshot.
pub(crate) fn era_validators_from_legacy_snapshot(
    snapshot: SeigniorageRecipientsSnapshotV1,
) -> EraValidators {
    snapshot
        .into_iter()
        .map(|(era_id, recipients)| {
            let validator_weights = recipients
                .into_iter()
                .filter_map(|(public_key, bid)| bid.total_stake().map(|stake| (public_key, stake)))
                .collect::<ValidatorWeights>();
            (era_id, validator_weights)
        })
        .collect()
}

/// Initializes the vesting schedule of provided bid if the provided timestamp is greater than
/// or equal to the bid's initial release timestamp and the bid is owned by a genesis
/// validator.
///
/// Returns `true` if the provided bid's vesting schedule was initialized.
pub fn process_with_vesting_schedule<P>(
    provider: &mut P,
    validator_bid: &mut ValidatorBid,
    timestamp_millis: u64,
    vesting_schedule_period_millis: u64,
) -> Result<bool, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    let validator_public_key = validator_bid.validator_public_key().clone();

    let delegator_bids = read_delegator_bids(provider, &validator_public_key)?;
    for mut delegator_bid in delegator_bids {
        let delegator_staked_amount = delegator_bid.staked_amount();
        let delegator_vesting_schedule = match delegator_bid.vesting_schedule_mut() {
            Some(vesting_schedule) => vesting_schedule,
            None => continue,
        };
        if timestamp_millis < delegator_vesting_schedule.initial_release_timestamp_millis() {
            continue;
        }
        if delegator_vesting_schedule
            .initialize_with_schedule(delegator_staked_amount, vesting_schedule_period_millis)
        {
            let delegator_bid_key = delegator_bid.bid_addr().into();
            provider.write_bid(
                delegator_bid_key,
                BidKind::Delegator(Box::new(delegator_bid)),
            )?;
        }
    }

    let validator_staked_amount = validator_bid.staked_amount();
    let validator_vesting_schedule = match validator_bid.vesting_schedule_mut() {
        Some(vesting_schedule) => vesting_schedule,
        None => return Ok(false),
    };
    if timestamp_millis < validator_vesting_schedule.initial_release_timestamp_millis() {
        Ok(false)
    } else {
        Ok(validator_vesting_schedule
            .initialize_with_schedule(validator_staked_amount, vesting_schedule_period_millis))
    }
}

/// Returns all delegators for imputed validator.
pub fn delegators<P>(
    provider: &mut P,
    validator_public_key: &PublicKey,
) -> Result<Vec<Box<DelegatorBid>>, Error>
where
    P: RuntimeProvider + ?Sized + StorageProvider,
{
    let mut ret = vec![];
    let bid_addr = BidAddr::from(validator_public_key.clone());
    let mut delegator_bid_keys = provider.get_keys_by_prefix(
        &bid_addr
            .delegated_account_prefix()
            .map_err(|_| Error::Serialization)?,
    )?;
    delegator_bid_keys.extend(
        provider.get_keys_by_prefix(
            &bid_addr
                .delegated_purse_prefix()
                .map_err(|_| Error::Serialization)?,
        )?,
    );

    for delegator_bid_key in delegator_bid_keys {
        let delegator = read_delegator_bid(provider, &delegator_bid_key)?;
        ret.push(delegator);
    }

    Ok(ret)
}

/// Returns all delegator slot reservations for given validator.
pub fn reservations<P>(
    provider: &mut P,
    validator_public_key: &PublicKey,
) -> Result<Vec<Box<Reservation>>, Error>
where
    P: RuntimeProvider + ?Sized + StorageProvider,
{
    let mut ret = vec![];
    let bid_addr = BidAddr::from(validator_public_key.clone());
    let mut reservation_bid_keys = provider.get_keys_by_prefix(
        &bid_addr
            .reserved_account_prefix()
            .map_err(|_| Error::Serialization)?,
    )?;
    reservation_bid_keys.extend(
        provider.get_keys_by_prefix(
            &bid_addr
                .reserved_purse_prefix()
                .map_err(|_| Error::Serialization)?,
        )?,
    );

    for reservation_bid_key in reservation_bid_keys {
        let reservation = read_reservation_bid(provider, &reservation_bid_key)?;
        ret.push(reservation);
    }

    Ok(ret)
}
