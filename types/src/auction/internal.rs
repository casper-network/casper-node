use crate::{
    auction::{Bids, Delegators},
    bytesrepr::{FromBytes, ToBytes},
    system_contract_errors::auction::{Error, Result},
    CLTyped,
};

use super::{
    providers::StorageProvider, EraId, EraValidators, SeigniorageRecipientsSnapshot, BIDS_KEY,
    DELEGATORS_KEY, ERA_ID_KEY, ERA_VALIDATORS_KEY, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
};

fn read_from<P, T>(provider: &mut P, name: &str) -> Result<T>
where
    P: StorageProvider + ?Sized,
    T: FromBytes + CLTyped,
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
{
    let key = provider.get_key(name).ok_or(Error::MissingKey)?;
    let uref = key.into_uref().ok_or(Error::InvalidKeyVariant)?;
    provider.write(uref, value)?;
    Ok(())
}

pub fn get_bids<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<Bids> {
    Ok(read_from(provider, BIDS_KEY)?)
}

pub fn set_bids<P: StorageProvider + ?Sized>(provider: &mut P, validators: Bids) -> Result<()> {
    write_to(provider, BIDS_KEY, validators)
}

pub fn get_delegators<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<Delegators> {
    read_from(provider, DELEGATORS_KEY)
}

pub fn set_delegators<P: StorageProvider + ?Sized>(
    provider: &mut P,
    delegators: Delegators,
) -> Result<()> {
    write_to(provider, DELEGATORS_KEY, delegators)
}

pub fn get_era_validators<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<EraValidators> {
    Ok(read_from(provider, ERA_VALIDATORS_KEY)?)
}

pub fn set_era_validators<P: StorageProvider + ?Sized>(
    provider: &mut P,
    era_validators: EraValidators,
) -> Result<()> {
    write_to(provider, ERA_VALIDATORS_KEY, era_validators)
}

pub fn get_era_id<P: StorageProvider + ?Sized>(provider: &mut P) -> Result<EraId> {
    Ok(read_from(provider, ERA_ID_KEY)?)
}

pub fn set_era_id<P: StorageProvider + ?Sized>(provider: &mut P, era_id: u64) -> Result<()> {
    write_to(provider, ERA_ID_KEY, era_id)
}

pub fn get_seigniorage_recipients_snapshot<P: StorageProvider + ?Sized>(
    provider: &mut P,
) -> Result<SeigniorageRecipientsSnapshot> {
    Ok(read_from(provider, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)?)
}

pub fn set_seigniorage_recipients_snapshot<P: StorageProvider + ?Sized>(
    provider: &mut P,
    snapshot: SeigniorageRecipientsSnapshot,
) -> Result<()> {
    write_to(provider, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, snapshot)
}
