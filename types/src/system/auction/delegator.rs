// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{
    auction::bid::VestingSchedule,
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::Error,
    CLType, CLTyped, PublicKey, URef, U512,
};

/// Represents a party delegating their stake to a validator (or "delegatee")
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegator {
    staked_amount: U512,
    bonding_purse: URef,
    delegatee: PublicKey,
    vesting_schedule: Option<VestingSchedule>,
}

impl Delegator {
    /// Creates a new [`Delegator`]
    pub fn unlocked(staked_amount: U512, bonding_purse: URef, delegatee: PublicKey) -> Self {
        let vesting_schedule = None;
        Delegator {
            staked_amount,
            bonding_purse,
            delegatee,
            vesting_schedule,
        }
    }

    /// Creates new instance of a [`Delegator`] with locked funds.
    pub fn locked(
        staked_amount: U512,
        bonding_purse: URef,
        delegatee: PublicKey,
        release_timestamp_millis: u64,
    ) -> Self {
        let vesting_schedule = Some(VestingSchedule::new(release_timestamp_millis));
        Delegator {
            staked_amount,
            bonding_purse,
            delegatee,
            vesting_schedule,
        }
    }

    /// Returns the staked amount
    pub fn staked_amount(&self) -> &U512 {
        &self.staked_amount
    }

    /// Returns the bonding purse
    pub fn bonding_purse(&self) -> &URef {
        &self.bonding_purse
    }

    /// Returns delegatee
    pub fn delegatee(&self) -> &PublicKey {
        &self.delegatee
    }

    /// Decreases the stake of the provided bid
    pub fn decrease_stake(&mut self, amount: U512) -> Result<U512, Error> {
        let updated_staked_amount = self
            .staked_amount
            .checked_sub(amount)
            .ok_or(Error::InvalidAmount)?;

        self.staked_amount = updated_staked_amount;

        Ok(updated_staked_amount)
    }

    /// Increases the stake of the provided bid
    pub fn increase_stake(&mut self, amount: U512) -> Result<U512, Error> {
        let updated_staked_amount = self
            .staked_amount
            .checked_add(amount)
            .ok_or(Error::InvalidAmount)?;

        self.staked_amount = updated_staked_amount;

        Ok(updated_staked_amount)
    }
}

impl CLTyped for Delegator {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for Delegator {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.staked_amount.to_bytes()?);
        buffer.extend(self.bonding_purse.to_bytes()?);
        buffer.extend(self.delegatee.to_bytes()?);
        buffer.extend(self.vesting_schedule.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.staked_amount.serialized_length()
            + self.bonding_purse.serialized_length()
            + self.delegatee.serialized_length()
            + self.vesting_schedule.serialized_length()
    }
}

impl FromBytes for Delegator {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (staked_amount, bytes) = U512::from_bytes(bytes)?;
        let (bonding_purse, bytes) = URef::from_bytes(bytes)?;
        let (delegatee, bytes) = PublicKey::from_bytes(bytes)?;
        let (vesting_schedule, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            Delegator {
                staked_amount,
                bonding_purse,
                delegatee,
                vesting_schedule,
            },
            bytes,
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{auction::Delegator, bytesrepr, AccessRights, SecretKey, URef, U512};

    #[test]
    fn serialization_roundtrip() {
        let staked_amount = U512::one();
        let bonding_purse = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
        let delegatee = SecretKey::ed25519([43; 32]).into();
        let unlocked_delegator = Delegator::unlocked(staked_amount, bonding_purse, delegatee);
        bytesrepr::test_serialization_roundtrip(&unlocked_delegator);

        let release_timestamp_millis = 42;
        let locked_delegator = Delegator::locked(
            staked_amount,
            bonding_purse,
            delegatee,
            release_timestamp_millis,
        );
        bytesrepr::test_serialization_roundtrip(&locked_delegator);
    }
}
