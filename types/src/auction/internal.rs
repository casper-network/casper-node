use crate::{
    account::AccountHash,
    auction::{ActiveBids, DelegationsMap, FoundingValidators},
    bytesrepr::{FromBytes, ToBytes},
    system_contract_errors::auction::{Error, Result},
    CLTyped,
};

use super::{
    providers::StorageProvider, EraId, EraValidators, SeigniorageRecipientsSnapshot,
    ACTIVE_BIDS_KEY, DELEGATORS_KEY, ERA_ID_KEY, ERA_VALIDATORS_KEY, FOUNDING_VALIDATORS_KEY,
    SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    delegator::{RewardPerStakeMap, TallyMap, TotalDelegatorStakeMap, DelegatorRewardPoolMap},
};

fn read_from<P, T>(provider: &mut P, name: &str) -> Result<T>
where
    P: StorageProvider + ?Sized,
    T: FromBytes + CLTyped,
    Error: From<P::Error>,
{
    let key = provider.get_key(name).ok_or(Error::MissingKey)?;
    let uref = key.into_uref().ok_or(Error::InvalidKeyVariant)?;
    let value: T = provider.read(uref)?.ok_or(Error::MissingValue)?;
    Ok(value)
}

fn write_to<P, T>(provider: &mut P, name: &str, value: T) -> Result<()>
where
    P: StorageProvider + ?Sized,
    T: ToBytes + CLTyped,
    Error: From<P::Error>,
{
    let key = provider.get_key(name).ok_or(Error::MissingKey)?;
    let uref = key.into_uref().ok_or(Error::InvalidKeyVariant)?;
    provider.write(uref, value)?;
    Ok(())
}

pub fn get_founding_validators<P: StorageProvider + ?Sized>(
    provider: &mut P,
) -> Result<FoundingValidators>
where
    Error: From<P::Error>,
{
    Ok(read_from(provider, FOUNDING_VALIDATORS_KEY)?)
}

pub fn set_founding_validators<P: StorageProvider + ?Sized>(
    provider: &mut P,
    founding_validators: FoundingValidators,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, FOUNDING_VALIDATORS_KEY, founding_validators)
}

pub fn get_active_bids<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<ActiveBids>
where
    Error: From<P::Error>,
{
    Ok(read_from(provider, ACTIVE_BIDS_KEY)?)
}

pub fn set_active_bids<P: StorageProvider + ?Sized>(
    provider: &mut P,
    active_bids: ActiveBids,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, ACTIVE_BIDS_KEY, active_bids)
}

pub fn get_delegations_map<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<DelegationsMap>
where
    Error: From<P::Error>,
{
    read_from(provider, DELEGATIONS_MAP_KEY)
}

pub fn set_delegations_map<P: StorageProvider + ?Sized>(
    provider: &mut P,
    delegations_map: DelegationsMap,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, DELEGATIONS_MAP_KEY, delegations_map)
}

pub fn get_tally_map<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<TallyMap>
where
    Error: From<P::Error>,
{
    read_from(provider, TALLY_MAP_KEY)
}

pub fn set_tally_map<P: StorageProvider + ?Sized>(
    provider: &mut P,
    tally: TallyMap,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, TALLY_MAP_KEY, tally)
}

pub fn get_reward_per_stake_map<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<RewardPerStakeMap>
where
    Error: From<P::Error>,
{
    read_from(provider, REWARD_PER_STAKE_MAP_KEY)
}

pub fn set_reward_per_stake_map<P: StorageProvider + ?Sized>(
    provider: &mut P,
    reward_per_stake: RewardPerStakeMap,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, REWARD_PER_STAKE_MAP_KEY, reward_per_stake)
}

pub fn get_total_delegator_stake_map<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<TotalDelegatorStakeMap>
where
    Error: From<P::Error>,
{
    read_from(provider, TOTAL_DELEGATOR_STAKE_MAP_KEY)
}

pub fn set_total_delegator_stake_map<P: StorageProvider + ?Sized>(
    provider: &mut P,
    total_delegator_stake: TotalDelegatorStakeMap,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, TOTAL_DELEGATOR_STAKE_MAP_KEY, total_delegator_stake)
}

pub fn get_delegator_reward_pool_map<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<DelegatorRewardPoolMap>
where
    Error: From<P::Error>,
{
    read_from(provider, DELEGATOR_REWARD_POOL_MAP)
}

pub fn set_delegator_reward_pool_map<P: StorageProvider + ?Sized>(
    provider: &mut P,
    delegator_reward_pool_map: DelegatorRewardPoolMap,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, DELEGATOR_REWARD_POOL_MAP, delegator_reward_pool_map)
}

pub fn get_era_validators<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<EraValidators>
where
    Error: From<P::Error>,
{
    Ok(read_from(provider, ERA_VALIDATORS_KEY)?)
}

pub fn set_era_validators<P: StorageProvider + ?Sized>(
    provider: &mut P,
    era_validators: EraValidators,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, ERA_VALIDATORS_KEY, era_validators)
}

pub fn get_era_id<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<EraId>
where
    Error: From<P::Error>,
{
    Ok(read_from(provider, ERA_ID_KEY)?)
}

pub fn set_era_id<P: StorageProvider + ?Sized>(provider: &mut P, era_id: u64) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, ERA_ID_KEY, era_id)
}

pub fn get_seigniorage_recipients_snapshot<P: StorageProvider + ?Sized>(
    provider: &mut P,
) -> Result<SeigniorageRecipientsSnapshot>
where
    Error: From<P::Error>,
{
    Ok(read_from(provider, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)?)
}

pub fn set_seigniorage_recipients_snapshot<P: StorageProvider + ?Sized>(
    provider: &mut P,
    snapshot: SeigniorageRecipientsSnapshot,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, snapshot)
}
