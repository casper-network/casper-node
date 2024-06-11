// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::vec::Vec;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::{
        bid::VestingSchedule, Bid, DelegationRate, Error, VESTING_SCHEDULE_LENGTH_MILLIS,
    },
    CLType, CLTyped, PublicKey, URef, U512,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// An entry in the validator map.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ValidatorBid {
    /// Validator public key
    validator_public_key: PublicKey,
    /// The purse that was used for bonding.
    bonding_purse: URef,
    /// The amount of tokens staked by a validator (not including delegators).
    staked_amount: U512,
    /// Delegation rate
    delegation_rate: DelegationRate,
    /// Vesting schedule for a genesis validator. `None` if non-genesis validator.
    vesting_schedule: Option<VestingSchedule>,
    /// `true` if validator has been "evicted"
    inactive: bool,
    /// Minimum allowed delegation amount in motes
    minimum_delegation_amount: u64,
    /// Maximum allowed delegation amount in motes
    maximum_delegation_amount: u64,
}

impl ValidatorBid {
    /// Creates new instance of a bid with locked funds.
    pub fn locked(
        validator_public_key: PublicKey,
        bonding_purse: URef,
        staked_amount: U512,
        delegation_rate: DelegationRate,
        release_timestamp_millis: u64,
        minimum_delegation_amount: u64,
        maximum_delegation_amount: u64,
    ) -> Self {
        let vesting_schedule = Some(VestingSchedule::new(release_timestamp_millis));
        let inactive = false;
        Self {
            validator_public_key,
            bonding_purse,
            staked_amount,
            delegation_rate,
            vesting_schedule,
            inactive,
            minimum_delegation_amount,
            maximum_delegation_amount,
        }
    }

    /// Creates new instance of a bid with unlocked funds.
    pub fn unlocked(
        validator_public_key: PublicKey,
        bonding_purse: URef,
        staked_amount: U512,
        delegation_rate: DelegationRate,
        minimum_delegation_amount: u64,
        maximum_delegation_amount: u64,
    ) -> Self {
        let vesting_schedule = None;
        let inactive = false;
        Self {
            validator_public_key,
            bonding_purse,
            staked_amount,
            delegation_rate,
            vesting_schedule,
            inactive,
            minimum_delegation_amount,
            maximum_delegation_amount,
        }
    }

    /// Creates a new inactive instance of a bid with 0 staked amount.
    pub fn empty(validator_public_key: PublicKey, bonding_purse: URef) -> Self {
        let vesting_schedule = None;
        let inactive = true;
        let staked_amount = U512::zero();
        let delegation_rate = Default::default();
        Self {
            validator_public_key,
            bonding_purse,
            staked_amount,
            delegation_rate,
            vesting_schedule,
            inactive,
            minimum_delegation_amount: 0,
            maximum_delegation_amount: u64::MAX,
        }
    }

    /// Gets the validator public key of the provided bid
    pub fn validator_public_key(&self) -> &PublicKey {
        &self.validator_public_key
    }

    /// Gets the bonding purse of the provided bid
    pub fn bonding_purse(&self) -> &URef {
        &self.bonding_purse
    }

    /// Checks if a bid is still locked under a vesting schedule.
    ///
    /// Returns true if a timestamp falls below the initial lockup period + 91 days release
    /// schedule, otherwise false.
    pub fn is_locked(&self, timestamp_millis: u64) -> bool {
        self.is_locked_with_vesting_schedule(timestamp_millis, VESTING_SCHEDULE_LENGTH_MILLIS)
    }

    /// Checks if a bid is still locked under a vesting schedule.
    ///
    /// Returns true if a timestamp falls below the initial lockup period + 91 days release
    /// schedule, otherwise false.
    pub fn is_locked_with_vesting_schedule(
        &self,
        timestamp_millis: u64,
        vesting_schedule_period_millis: u64,
    ) -> bool {
        match &self.vesting_schedule {
            Some(vesting_schedule) => {
                vesting_schedule.is_vesting(timestamp_millis, vesting_schedule_period_millis)
            }
            None => false,
        }
    }

    /// Gets the staked amount of the provided bid
    pub fn staked_amount(&self) -> U512 {
        self.staked_amount
    }

    /// Gets the staked amount of the provided bid
    pub fn staked_amount_mut(&mut self) -> &mut U512 {
        &mut self.staked_amount
    }

    /// Gets the delegation rate of the provided bid
    pub fn delegation_rate(&self) -> &DelegationRate {
        &self.delegation_rate
    }

    /// Returns a reference to the vesting schedule of the provided bid.  `None` if a non-genesis
    /// validator.
    pub fn vesting_schedule(&self) -> Option<&VestingSchedule> {
        self.vesting_schedule.as_ref()
    }

    /// Returns a mutable reference to the vesting schedule of the provided bid.  `None` if a
    /// non-genesis validator.
    pub fn vesting_schedule_mut(&mut self) -> Option<&mut VestingSchedule> {
        self.vesting_schedule.as_mut()
    }

    /// Returns `true` if validator is inactive
    pub fn inactive(&self) -> bool {
        self.inactive
    }

    /// Decreases the stake of the provided bid
    pub fn decrease_stake(
        &mut self,
        amount: U512,
        era_end_timestamp_millis: u64,
    ) -> Result<U512, Error> {
        let updated_staked_amount = self
            .staked_amount
            .checked_sub(amount)
            .ok_or(Error::UnbondTooLarge)?;

        let vesting_schedule = match self.vesting_schedule.as_ref() {
            Some(vesting_schedule) => vesting_schedule,
            None => {
                self.staked_amount = updated_staked_amount;
                return Ok(updated_staked_amount);
            }
        };

        match vesting_schedule.locked_amount(era_end_timestamp_millis) {
            Some(locked_amount) if updated_staked_amount < locked_amount => {
                Err(Error::ValidatorFundsLocked)
            }
            None => {
                // If `None`, then the locked amounts table has yet to be initialized (likely
                // pre-90 day mark)
                Err(Error::ValidatorFundsLocked)
            }
            Some(_) => {
                self.staked_amount = updated_staked_amount;
                Ok(updated_staked_amount)
            }
        }
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

    /// Updates the delegation rate of the provided bid
    pub fn with_delegation_rate(&mut self, delegation_rate: DelegationRate) -> &mut Self {
        self.delegation_rate = delegation_rate;
        self
    }

    /// Sets given bid's `inactive` field to `false`
    pub fn activate(&mut self) -> bool {
        self.inactive = false;
        false
    }

    /// Sets given bid's `inactive` field to `true`
    pub fn deactivate(&mut self) -> bool {
        self.inactive = true;
        true
    }

    /// Sets validator public key
    pub fn with_validator_public_key(&mut self, validator_public_key: PublicKey) -> &mut Self {
        self.validator_public_key = validator_public_key;
        self
    }

    /// Returns minimum allowed delegation amount in motes.
    pub fn minimum_delegation_amount(&self) -> u64 {
        self.minimum_delegation_amount
    }

    /// Returns maximum allowed delegation amount in motes.
    pub fn maximum_delegation_amount(&self) -> u64 {
        self.maximum_delegation_amount
    }

    /// Sets minimum and maximum delegation amounts in motes.
    pub fn set_delegation_amount_boundaries(
        &mut self,
        minimum_delegation_amount: u64,
        maximum_delegation_amount: u64,
    ) {
        self.minimum_delegation_amount = minimum_delegation_amount;
        self.maximum_delegation_amount = maximum_delegation_amount;
    }
}

impl CLTyped for ValidatorBid {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for ValidatorBid {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.validator_public_key.write_bytes(&mut result)?;
        self.bonding_purse.write_bytes(&mut result)?;
        self.staked_amount.write_bytes(&mut result)?;
        self.delegation_rate.write_bytes(&mut result)?;
        self.vesting_schedule.write_bytes(&mut result)?;
        self.inactive.write_bytes(&mut result)?;
        self.minimum_delegation_amount.write_bytes(&mut result)?;
        self.maximum_delegation_amount.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.validator_public_key.serialized_length()
            + self.bonding_purse.serialized_length()
            + self.staked_amount.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.vesting_schedule.serialized_length()
            + self.inactive.serialized_length()
            + self.minimum_delegation_amount.serialized_length()
            + self.maximum_delegation_amount.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.validator_public_key.write_bytes(writer)?;
        self.bonding_purse.write_bytes(writer)?;
        self.staked_amount.write_bytes(writer)?;
        self.delegation_rate.write_bytes(writer)?;
        self.vesting_schedule.write_bytes(writer)?;
        self.inactive.write_bytes(writer)?;
        self.minimum_delegation_amount.write_bytes(writer)?;
        self.maximum_delegation_amount.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for ValidatorBid {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (validator_public_key, bytes) = FromBytes::from_bytes(bytes)?;
        let (bonding_purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (staked_amount, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (vesting_schedule, bytes) = FromBytes::from_bytes(bytes)?;
        let (inactive, bytes) = FromBytes::from_bytes(bytes)?;
        let (minimum_delegation_amount, bytes) = FromBytes::from_bytes(bytes)?;
        let (maximum_delegation_amount, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            ValidatorBid {
                validator_public_key,
                bonding_purse,
                staked_amount,
                delegation_rate,
                vesting_schedule,
                inactive,
                minimum_delegation_amount,
                maximum_delegation_amount,
            },
            bytes,
        ))
    }
}

impl From<Bid> for ValidatorBid {
    fn from(bid: Bid) -> Self {
        ValidatorBid {
            validator_public_key: bid.validator_public_key().clone(),
            bonding_purse: *bid.bonding_purse(),
            staked_amount: *bid.staked_amount(),
            delegation_rate: *bid.delegation_rate(),
            vesting_schedule: bid.vesting_schedule().cloned(),
            inactive: bid.inactive(),
            minimum_delegation_amount: 0,
            maximum_delegation_amount: u64::MAX,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        bytesrepr,
        system::auction::{bid::VestingSchedule, DelegationRate, ValidatorBid},
        AccessRights, PublicKey, SecretKey, URef, U512,
    };

    #[test]
    fn serialization_roundtrip_active() {
        let founding_validator = ValidatorBid {
            validator_public_key: PublicKey::from(
                &SecretKey::ed25519_from_bytes([0u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            bonding_purse: URef::new([42; 32], AccessRights::READ_ADD_WRITE),
            staked_amount: U512::one(),
            delegation_rate: DelegationRate::MAX,
            vesting_schedule: Some(VestingSchedule::default()),
            inactive: false,
            minimum_delegation_amount: 0,
            maximum_delegation_amount: u64::MAX,
        };
        bytesrepr::test_serialization_roundtrip(&founding_validator);
    }

    #[test]
    fn serialization_roundtrip_inactive() {
        let founding_validator = ValidatorBid {
            validator_public_key: PublicKey::from(
                &SecretKey::ed25519_from_bytes([0u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            bonding_purse: URef::new([42; 32], AccessRights::READ_ADD_WRITE),
            staked_amount: U512::one(),
            delegation_rate: DelegationRate::max_value(),
            vesting_schedule: Some(VestingSchedule::default()),
            inactive: true,
            minimum_delegation_amount: 0,
            maximum_delegation_amount: u64::MAX,
        };
        bytesrepr::test_serialization_roundtrip(&founding_validator);
    }

    #[test]
    fn should_immediately_initialize_unlock_amounts() {
        const TIMESTAMP_MILLIS: u64 = 0;

        let validator_pk: PublicKey = (&SecretKey::ed25519_from_bytes([42; 32]).unwrap()).into();

        let validator_release_timestamp = TIMESTAMP_MILLIS;
        let vesting_schedule_period_millis = TIMESTAMP_MILLIS;
        let validator_bonding_purse = URef::new([42; 32], AccessRights::ADD);
        let validator_staked_amount = U512::from(1000);
        let validator_delegation_rate = 0;

        let bid = ValidatorBid::locked(
            validator_pk,
            validator_bonding_purse,
            validator_staked_amount,
            validator_delegation_rate,
            validator_release_timestamp,
            0,
            u64::MAX,
        );

        assert!(!bid.is_locked_with_vesting_schedule(
            validator_release_timestamp,
            vesting_schedule_period_millis
        ));
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid(bid in gens::validator_bid_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid);
        }
    }
}
