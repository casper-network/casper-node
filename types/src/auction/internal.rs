use crate::{
    auction::{ActiveBids, Delegators, FoundingValidators},
    bytesrepr::{FromBytes, ToBytes},
    system_contract_errors::auction::{Error, Result},
    CLTyped,
};

use super::{
    providers::StorageProvider, EraValidators, ACTIVE_BIDS_KEY, DELEGATORS_KEY, ERA_INDEX_KEY,
    ERA_VALIDATORS_KEY, FOUNDING_VALIDATORS_KEY,
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

pub fn get_delegators<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<Delegators>
where
    Error: From<P::Error>,
{
    read_from(provider, DELEGATORS_KEY)
}

pub fn set_delegators<P: StorageProvider + ?Sized>(
    provider: &mut P,
    delegators: Delegators,
) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, DELEGATORS_KEY, delegators)
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

pub fn get_era_index<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<u64>
where
    Error: From<P::Error>,
{
    Ok(read_from(provider, ERA_INDEX_KEY)?)
}

pub fn set_era_index<P: StorageProvider + ?Sized>(provider: &mut P, era_index: u64) -> Result<()>
where
    Error: From<P::Error>,
{
    write_to(provider, ERA_INDEX_KEY, era_index)
}
