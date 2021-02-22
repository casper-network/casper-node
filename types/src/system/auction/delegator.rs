// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::vec::Vec;

#[cfg(feature = "std")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::Error,
    CLType, CLTyped, PublicKey, URef, U512,
};

/// Represents a party delegating their stake to a validator (or "delegatee")
#[derive(Debug, Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "std", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Delegator {
    delegator_public_key: PublicKey,
    staked_amount: U512,
    bonding_purse: URef,
    validator_public_key: PublicKey,
}

impl Delegator {
    /// Creates a new [`Delegator`]
    pub fn new(
        delegator_public_key: PublicKey,
        staked_amount: U512,
        bonding_purse: URef,
        validator_public_key: PublicKey,
    ) -> Self {
        Delegator {
            delegator_public_key,
            staked_amount,
            bonding_purse,
            validator_public_key,
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
        buffer.extend(self.delegator_public_key.to_bytes()?);
        buffer.extend(self.staked_amount.to_bytes()?);
        buffer.extend(self.bonding_purse.to_bytes()?);
        buffer.extend(self.validator_public_key.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.delegator_public_key.serialized_length()
            + self.staked_amount.serialized_length()
            + self.bonding_purse.serialized_length()
            + self.validator_public_key.serialized_length()
    }
}

impl FromBytes for Delegator {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (delegator_public_key, bytes) = PublicKey::from_bytes(bytes)?;
        let (staked_amount, bytes) = U512::from_bytes(bytes)?;
        let (bonding_purse, bytes) = URef::from_bytes(bytes)?;
        let (validator_public_key, bytes) = PublicKey::from_bytes(bytes)?;
        Ok((
            Delegator {
                delegator_public_key,
                staked_amount,
                bonding_purse,
                validator_public_key,
            },
            bytes,
        ))
    }
}
