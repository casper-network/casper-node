use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system_contract_errors::auction::Error,
    CLType, CLTyped, PublicKey, URef, U512,
};

/// Represents a party delegating their stake to a validator (or "delegatee")
#[derive(Debug, Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegator {
    staked_amount: U512,
    bonding_purse: URef,
    delegatee: PublicKey,
    reward: U512,
}

impl Delegator {
    /// Creates a new [`Delegator`]
    pub fn new(staked_amount: U512, bonding_purse: URef, delegatee: PublicKey) -> Self {
        let reward = U512::zero();
        Delegator {
            staked_amount,
            bonding_purse,
            delegatee,
            reward,
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

    /// Returns the seigniorage reward
    pub fn reward(&self) -> &U512 {
        &self.reward
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

    /// Increases the seigniorage reward of the provided delegator
    pub fn increase_reward(&mut self, amount: U512) -> Result<U512, Error> {
        let updated_reward = self
            .reward
            .checked_add(amount)
            .ok_or(Error::InvalidAmount)?;

        self.reward = updated_reward;

        Ok(updated_reward)
    }

    /// Zeros the seigniorage reward of the provided delegator
    pub fn zero_reward(&mut self) {
        self.reward = U512::zero()
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
        buffer.extend(self.reward.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.staked_amount.serialized_length()
            + self.bonding_purse.serialized_length()
            + self.delegatee.serialized_length()
            + self.reward.serialized_length()
    }
}

impl FromBytes for Delegator {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (staked_amount, bytes) = U512::from_bytes(bytes)?;
        let (bonding_purse, bytes) = URef::from_bytes(bytes)?;
        let (delegatee, bytes) = PublicKey::from_bytes(bytes)?;
        let (reward, bytes) = U512::from_bytes(bytes)?;
        Ok((
            Delegator {
                staked_amount,
                bonding_purse,
                delegatee,
                reward,
            },
            bytes,
        ))
    }
}
