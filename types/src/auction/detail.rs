use alloc::vec::Vec;

use num_rational::Ratio;

use super::{
    Auction, BidPurses, UnbondingPurses, BID_PURSES_KEY, SYSTEM_ACCOUNT, UNBONDING_PURSES_KEY,
};
use crate::{
    auction::{internal, MintProvider, RuntimeProvider, StorageProvider, SystemProvider},
    system_contract_errors::auction::{Error, Result},
    Key, PublicKey, U512,
};

/// Iterates over unbonding entries and checks if a locked amount can be paid already if
/// a specific era is reached.
///
/// This function can be called by the system only.
pub(crate) fn process_unbond_requests<P: Auction + ?Sized>(provider: &mut P) -> Result<()> {
    if provider.get_caller() != SYSTEM_ACCOUNT {
        return Err(Error::InvalidCaller);
    }
    let bid_purses_uref = provider
        .get_key(BID_PURSES_KEY)
        .and_then(Key::into_uref)
        .ok_or(Error::MissingKey)?;

    let bid_purses: BidPurses = provider.read(bid_purses_uref)?.ok_or(Error::Storage)?;

    // Update `unbonding_purses` data
    let unbonding_purses_uref = provider
        .get_key(UNBONDING_PURSES_KEY)
        .and_then(Key::into_uref)
        .ok_or(Error::MissingKey)?;
    let mut unbonding_purses: UnbondingPurses = provider
        .read(unbonding_purses_uref)?
        .ok_or(Error::Storage)?;

    let current_era_id = provider.read_era_id()?;

    for unbonding_list in unbonding_purses.values_mut() {
        let mut new_unbonding_list = Vec::new();
        for unbonding_purse in unbonding_list.iter() {
            let source = bid_purses
                .get(&unbonding_purse.origin)
                .ok_or(Error::BondNotFound)?;
            // Since `process_unbond_requests` is run before `run_auction`, we should check
            // if current era id is equal or greater than the `era_of_withdrawal` that was
            // calculated on `unbond` attempt.
            if current_era_id >= unbonding_purse.era_of_withdrawal as u64 {
                // Move funds from bid purse to unbonding purse
                provider.transfer_from_purse_to_purse(
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

    // Prune empty entries
    let new_unbonding_purses: UnbondingPurses = unbonding_purses
        .into_iter()
        .filter(|(_k, unbonding_purses)| !unbonding_purses.is_empty())
        .collect();

    provider.write(unbonding_purses_uref, new_unbonding_purses)?;
    Ok(())
}

/// Update delegators entry. Initialize if it doesn't exist.
pub fn update_delegators<P>(
    provider: &mut P,
    validator_public_key: PublicKey,
    delegator_public_key: PublicKey,
    delegation_amount: U512,
) -> Result<U512>
where
    P: RuntimeProvider + StorageProvider + ?Sized,
{
    let mut delegators = internal::get_delegators(provider)?;
    let new_quantity = *delegators
        .entry(validator_public_key)
        .or_default()
        .entry(delegator_public_key)
        .and_modify(|delegation| *delegation += delegation_amount)
        .or_insert_with(|| delegation_amount);
    internal::set_delegators(provider, delegators)?;
    Ok(new_quantity)
}

/// Update validator reward map.
pub fn update_delegator_rewards<P>(
    provider: &mut P,
    validator_public_key: PublicKey,
    rewards: impl Iterator<Item = (PublicKey, Ratio<U512>)>,
) -> Result<U512>
where
    P: MintProvider + RuntimeProvider + StorageProvider + SystemProvider + ?Sized,
{
    let mut total_delegator_payout = U512::zero();
    let mut outer = internal::get_delegator_reward_map(provider)?;
    let mut inner = outer.remove(&validator_public_key).unwrap_or_default();

    for (delegator_key, delegator_reward) in rewards {
        let delegator_reward_trunc = delegator_reward.to_integer();
        inner
            .entry(delegator_key)
            .and_modify(|sum| *sum += delegator_reward_trunc)
            .or_insert_with(|| delegator_reward_trunc);
        total_delegator_payout += delegator_reward_trunc;
    }

    outer.insert(validator_public_key, inner);
    internal::set_delegator_reward_map(provider, outer)?;
    Ok(total_delegator_payout)
}

/// Update validator reward map.
pub fn update_validator_reward<P>(
    provider: &mut P,
    validator_public_key: PublicKey,
    amount: U512,
) -> Result<()>
where
    P: MintProvider + RuntimeProvider + StorageProvider + SystemProvider + ?Sized,
{
    let mut validator_reward_map = internal::get_validator_reward_map(provider)?;
    validator_reward_map
        .entry(validator_public_key)
        .and_modify(|sum| *sum += amount)
        .or_insert_with(|| amount);
    internal::set_validator_reward_map(provider, validator_reward_map)?;
    Ok(())
}
