mod vesting;

use alloc::{collections::BTreeMap, vec::Vec};

use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::{DelegationRate, Delegator, Error},
    CLType, CLTyped, PublicKey, URef, U512,
};

pub use vesting::VestingSchedule;

/// An entry in a founding validator map.
#[derive(PartialEq, Debug, Serialize, Deserialize, Clone)]
pub struct Bid {
    /// The purse that was used for bonding.
    bonding_purse: URef,
    /// The amount of tokens staked by a validator (not including delegators).
    staked_amount: U512,
    /// Delegation rate
    delegation_rate: DelegationRate,
    /// Vesting schedule for a genesis validator. `None` if non-genesis validator.
    vesting_schedule: Option<VestingSchedule>,
    /// This validator's delegators, indexed by their public keys
    delegators: BTreeMap<PublicKey, Delegator>,
    /// `true` if validator has been "evicted"
    inactive: bool,
}

impl Bid {
    /// Creates new instance of a bid with locked funds.
    pub fn locked(bonding_purse: URef, staked_amount: U512, release_timestamp_millis: u64) -> Self {
        let delegation_rate = 0;
        let vesting_schedule = Some(VestingSchedule::new(release_timestamp_millis));
        let delegators = BTreeMap::new();
        let inactive = false;
        Self {
            bonding_purse,
            staked_amount,
            delegation_rate,
            vesting_schedule,
            delegators,
            inactive,
        }
    }

    /// Creates new instance of a bid with unlocked funds.
    pub fn unlocked(
        bonding_purse: URef,
        staked_amount: U512,
        delegation_rate: DelegationRate,
    ) -> Self {
        let vesting_schedule = None;
        let delegators = BTreeMap::new();
        let inactive = false;
        Self {
            bonding_purse,
            staked_amount,
            delegation_rate,
            vesting_schedule,
            delegators,
            inactive,
        }
    }

    /// Gets the bonding purse of the provided bid
    pub fn bonding_purse(&self) -> &URef {
        &self.bonding_purse
    }

    /// Gets the staked amount of the provided bid
    pub fn staked_amount(&self) -> &U512 {
        &self.staked_amount
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

    /// Returns a reference to the delegators of the provided bid
    pub fn delegators(&self) -> &BTreeMap<PublicKey, Delegator> {
        &self.delegators
    }

    /// Returns a mutable reference to the delegators of the provided bid
    pub fn delegators_mut(&mut self) -> &mut BTreeMap<PublicKey, Delegator> {
        &mut self.delegators
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
            Some(vesting_sechdule) => vesting_sechdule,
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

    /// Initializes the vesting schedule of provided bid if the provided timestamp is greater than
    /// or equal to the bid's initial release timestamp and the bid is owned by a genesis
    /// validator.
    ///
    /// Returns `true` if the provided bid's vesting schedule was initialized.
    pub fn process(&mut self, timestamp_millis: u64) -> bool {
        // Put timestamp-sensitive processing logic in here
        let staked_amount = self.staked_amount;
        let vesting_schedule = match self.vesting_schedule_mut() {
            Some(vesting_schedule) => vesting_schedule,
            None => return false,
        };
        if timestamp_millis < vesting_schedule.initial_release_timestamp_millis() {
            return false;
        }
        vesting_schedule.initialize(staked_amount)
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

    /// Returns the total staked amount of validator + all delegators
    pub fn total_staked_amount(&self) -> Result<U512, Error> {
        self.delegators
            .iter()
            .fold(Some(U512::zero()), |maybe_a, (_, b)| {
                maybe_a.and_then(|a| a.checked_add(*b.staked_amount()))
            })
            .and_then(|delegators_sum| delegators_sum.checked_add(*self.staked_amount()))
            .ok_or(Error::InvalidAmount)
    }
}

impl CLTyped for Bid {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for Bid {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(self.bonding_purse.to_bytes()?);
        result.extend(self.staked_amount.to_bytes()?);
        result.extend(self.delegation_rate.to_bytes()?);
        result.extend(self.vesting_schedule.to_bytes()?);
        result.extend(self.delegators.to_bytes()?);
        result.extend(self.inactive.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.bonding_purse.serialized_length()
            + self.staked_amount.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.vesting_schedule.serialized_length()
            + self.delegators.serialized_length()
            + self.inactive.serialized_length()
    }
}

impl FromBytes for Bid {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bonding_purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (staked_amount, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (vesting_schedule, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegators, bytes) = FromBytes::from_bytes(bytes)?;
        let (inactive, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            Bid {
                bonding_purse,
                staked_amount,
                delegation_rate,
                vesting_schedule,
                delegators,
                inactive,
            },
            bytes,
        ))
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use crate::{
        bytesrepr,
        system::auction::{bid::VestingSchedule, Bid, DelegationRate},
        AccessRights, URef, U512,
    };

    #[test]
    fn serialization_roundtrip() {
        let founding_validator = Bid {
            bonding_purse: URef::new([42; 32], AccessRights::READ_ADD_WRITE),
            staked_amount: U512::one(),
            delegation_rate: DelegationRate::max_value(),
            vesting_schedule: Some(VestingSchedule::default()),
            delegators: BTreeMap::default(),
            inactive: true,
        };
        bytesrepr::test_serialization_roundtrip(&founding_validator);
    }
}
