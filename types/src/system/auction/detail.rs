use alloc::{collections::BTreeMap, vec::Vec};
use core::convert::TryInto;

use num_rational::Ratio;

use crate::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::auction::{
        constants::*, Auction, Bids, EraId, Error, RuntimeProvider, SeigniorageAllocation,
        SeigniorageRecipientsSnapshot, StorageProvider, UnbondingPurse, UnbondingPurses,
    },
    CLTyped, Key, KeyTag, PublicKey, URef, U512,
};

fn read_from<P, T>(provider: &mut P, name: &str) -> Result<T, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
    T: FromBytes + CLTyped,
{
    let key = provider.named_keys_get(name).ok_or(Error::MissingKey)?;
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
    provider.write(uref, value)?;
    Ok(())
}

pub fn get_bids<P>(provider: &mut P) -> Result<Bids, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    let bids_keys = provider.get_keys(&KeyTag::Bid)?;

    let mut ret = BTreeMap::new();

    for key in bids_keys {
        let account_hash = match key {
            Key::Bid(account_ash) => account_ash,
            _ => return Err(Error::InvalidKeyVariant),
        };
        let bid = match provider.read_bid(&account_hash)? {
            Some(bid) => bid,
            None => return Err(Error::ValidatorNotFound),
        };
        ret.insert(bid.validator_public_key().clone(), bid);
    }

    Ok(ret)
}

pub fn set_bids<P>(provider: &mut P, validators: Bids) -> Result<(), Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    for (_, bid) in validators.into_iter() {
        let account_hash = AccountHash::from(bid.validator_public_key());
        provider.write_bid(account_hash, bid)?;
    }
    Ok(())
}

pub fn get_unbonding_purses<P>(provider: &mut P) -> Result<UnbondingPurses, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    let withdraws_keys = provider.get_keys(&KeyTag::Withdraw)?;

    let mut ret = BTreeMap::new();

    for key in withdraws_keys {
        let account_hash = match key {
            Key::Withdraw(account_ash) => account_ash,
            _ => return Err(Error::InvalidKeyVariant),
        };
        let unbonding_purses = provider.read_withdraw(&account_hash)?;
        ret.insert(account_hash, unbonding_purses);
    }

    Ok(ret)
}

pub fn set_unbonding_purses<P>(
    provider: &mut P,
    unbonding_purses: UnbondingPurses,
) -> Result<(), Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    for (account_hash, unbonding_purses) in unbonding_purses.into_iter() {
        provider.write_withdraw(account_hash, unbonding_purses)?;
    }
    Ok(())
}

pub fn get_era_id<P>(provider: &mut P) -> Result<EraId, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    read_from(provider, ERA_ID_KEY)
}

pub fn set_era_id<P>(provider: &mut P, era_id: EraId) -> Result<(), Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    write_to(provider, ERA_ID_KEY, era_id)
}

pub fn get_era_end_timestamp_millis<P>(provider: &mut P) -> Result<u64, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    read_from(provider, ERA_END_TIMESTAMP_MILLIS_KEY)
}

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

pub fn get_seigniorage_recipients_snapshot<P>(
    provider: &mut P,
) -> Result<SeigniorageRecipientsSnapshot, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    read_from(provider, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)
}

pub fn set_seigniorage_recipients_snapshot<P>(
    provider: &mut P,
    snapshot: SeigniorageRecipientsSnapshot,
) -> Result<(), Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    write_to(provider, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, snapshot)
}

pub fn get_validator_slots<P>(provider: &mut P) -> Result<usize, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    let validator_slots: u32 = read_from(provider, VALIDATOR_SLOTS_KEY)?;
    let validator_slots = validator_slots
        .try_into()
        .map_err(|_| Error::InvalidValidatorSlotsValue)?;
    Ok(validator_slots)
}

pub fn get_auction_delay<P>(provider: &mut P) -> Result<u64, Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    let auction_delay: u64 = read_from(provider, AUCTION_DELAY_KEY)?;
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
pub(crate) fn process_unbond_requests<P: Auction + ?Sized>(provider: &mut P) -> Result<(), Error> {
    if provider.get_caller() != PublicKey::System.to_account_hash() {
        return Err(Error::InvalidCaller);
    }

    // Update `unbonding_purses` data
    let mut unbonding_purses: UnbondingPurses = get_unbonding_purses(provider)?;

    let current_era_id = provider.read_era_id()?;

    let unbonding_delay = get_unbonding_delay(provider)?;

    for unbonding_list in unbonding_purses.values_mut() {
        let mut new_unbonding_list = Vec::new();
        for unbonding_purse in unbonding_list.iter() {
            // Since `process_unbond_requests` is run before `run_auction`, we should check if
            // current era id + unbonding delay is equal or greater than the `era_of_creation` that
            // was calculated on `unbond` attempt.
            if current_era_id >= unbonding_purse.era_of_creation() + unbonding_delay {
                // Move funds from bid purse to unbonding purse
                provider
                    .unbond(unbonding_purse)
                    .map_err(|_| Error::TransferToUnbondingPurse)?;
            } else {
                new_unbonding_list.push(unbonding_purse.clone());
            }
        }
        *unbonding_list = new_unbonding_list;
    }

    set_unbonding_purses(provider, unbonding_purses)?;
    Ok(())
}

/// Creates a new purse in unbonding_purses given a validator's key, amount, and a destination
/// unbonding purse. Returns the amount of motes remaining in the validator's bid purse.
pub(crate) fn create_unbonding_purse<P: Auction + ?Sized>(
    provider: &mut P,
    validator_public_key: PublicKey,
    unbonder_public_key: PublicKey,
    bonding_purse: URef,
    amount: U512,
) -> Result<(), Error> {
    if provider.get_balance(bonding_purse)?.unwrap_or_default() < amount {
        return Err(Error::UnbondTooLarge);
    }

    let validator_account_hash = AccountHash::from(&validator_public_key);
    let mut unbonding_purses = provider.read_withdraw(&validator_account_hash)?;
    let era_of_creation = provider.read_era_id()?;
    let new_unbonding_purse = UnbondingPurse::new(
        bonding_purse,
        validator_public_key,
        unbonder_public_key,
        era_of_creation,
        amount,
    );
    unbonding_purses.push(new_unbonding_purse);
    provider.write_withdraw(validator_account_hash, unbonding_purses)?;

    Ok(())
}

/// Reinvests delegator reward by increasing its stake.
pub fn reinvest_delegator_rewards<P>(
    provider: &mut P,
    seigniorage_allocations: &mut Vec<SeigniorageAllocation>,
    validator_public_key: PublicKey,
    rewards: impl Iterator<Item = (PublicKey, Ratio<U512>)>,
) -> Result<Vec<(AccountHash, U512, URef)>, Error>
where
    P: StorageProvider,
{
    let mut delegator_payouts = Vec::new();

    let validator_account_hash = AccountHash::from(&validator_public_key);

    let mut bid = match provider.read_bid(&validator_account_hash)? {
        Some(bid) => bid,
        None => return Err(Error::ValidatorNotFound),
    };

    let delegators = bid.delegators_mut();

    for (delegator_key, delegator_reward) in rewards {
        let delegator = match delegators.get_mut(&delegator_key) {
            Some(delegator) => delegator,
            None => continue,
        };

        let delegator_reward_trunc = delegator_reward.to_integer();

        delegator.increase_stake(delegator_reward_trunc)?;

        delegator_payouts.push((
            delegator_key.to_account_hash(),
            delegator_reward_trunc,
            *delegator.bonding_purse(),
        ));

        let allocation = SeigniorageAllocation::delegator(
            delegator_key,
            validator_public_key.clone(),
            delegator_reward_trunc,
        );

        seigniorage_allocations.push(allocation);
    }

    provider.write_bid(validator_account_hash, bid)?;

    Ok(delegator_payouts)
}

/// Reinvests validator reward by increasing its stake and returns its bonding purse.
pub fn reinvest_validator_reward<P>(
    provider: &mut P,
    seigniorage_allocations: &mut Vec<SeigniorageAllocation>,
    validator_public_key: PublicKey,
    amount: U512,
) -> Result<URef, Error>
where
    P: StorageProvider,
{
    let validator_account_hash = AccountHash::from(&validator_public_key);

    let mut bid = match provider.read_bid(&validator_account_hash)? {
        Some(bid) => bid,
        None => {
            return Err(Error::ValidatorNotFound);
        }
    };

    bid.increase_stake(amount)?;

    let allocation = SeigniorageAllocation::validator(validator_public_key, amount);

    seigniorage_allocations.push(allocation);

    let bonding_purse = *bid.bonding_purse();

    provider.write_bid(validator_account_hash, bid)?;

    Ok(bonding_purse)
}
