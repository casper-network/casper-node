// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::{
        bid::VestingSchedule, delegator_kind::DelegatorKind, BidAddr, Delegator, Error, UnbondKind,
        VESTING_SCHEDULE_LENGTH_MILLIS,
    },
    CLType, CLTyped, PublicKey, URef, U512,
};
#[cfg(any(feature = "testing", test))]
use rand::Rng;

/// Represents a party delegating their stake to a validator (or "delegatee")
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DelegatorBid {
    delegator_kind: DelegatorKind,
    staked_amount: U512,
    bonding_purse: URef,
    validator_public_key: PublicKey,
    vesting_schedule: Option<VestingSchedule>,
}

impl DelegatorBid {
    /// Creates a new [`DelegatorBid`]
    pub fn unlocked(
        delegator_kind: DelegatorKind,
        staked_amount: U512,
        bonding_purse: URef,
        validator_public_key: PublicKey,
    ) -> Self {
        let vesting_schedule = None;
        DelegatorBid {
            delegator_kind,
            staked_amount,
            bonding_purse,
            validator_public_key,
            vesting_schedule,
        }
    }

    /// Creates new instance of a [`DelegatorBid`] with locked funds.
    pub fn locked(
        delegator_kind: DelegatorKind,
        staked_amount: U512,
        bonding_purse: URef,
        validator_public_key: PublicKey,
        release_timestamp_millis: u64,
    ) -> Self {
        let vesting_schedule = Some(VestingSchedule::new(release_timestamp_millis));
        DelegatorBid {
            delegator_kind,
            staked_amount,
            bonding_purse,
            validator_public_key,
            vesting_schedule,
        }
    }

    /// Returns the delegator kind.
    pub fn delegator_kind(&self) -> &DelegatorKind {
        &self.delegator_kind
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

    /// Returns the staked amount
    pub fn staked_amount(&self) -> U512 {
        self.staked_amount
    }

    /// Returns the mutable staked amount
    pub fn staked_amount_mut(&mut self) -> &mut U512 {
        &mut self.staked_amount
    }

    /// Returns the bonding purse
    pub fn bonding_purse(&self) -> &URef {
        &self.bonding_purse
    }

    /// Returns delegatee
    pub fn validator_public_key(&self) -> &PublicKey {
        &self.validator_public_key
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
            .ok_or(Error::InvalidAmount)?;

        let vesting_schedule = match self.vesting_schedule.as_ref() {
            Some(vesting_schedule) => vesting_schedule,
            None => {
                self.staked_amount = updated_staked_amount;
                return Ok(updated_staked_amount);
            }
        };

        if let Some(locked_amount) = vesting_schedule.locked_amount(era_end_timestamp_millis) {
            if updated_staked_amount < locked_amount {
                return Err(Error::DelegatorFundsLocked);
            }
        }
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

    /// Returns a reference to the vesting schedule of the provided
    /// delegator bid.  `None` if a non-genesis validator.
    pub fn vesting_schedule(&self) -> Option<&VestingSchedule> {
        self.vesting_schedule.as_ref()
    }

    /// Returns a mutable reference to the vesting schedule of the provided
    /// delegator bid.  `None` if a non-genesis validator.
    pub fn vesting_schedule_mut(&mut self) -> Option<&mut VestingSchedule> {
        self.vesting_schedule.as_mut()
    }

    /// Creates a new inactive instance of a bid with 0 staked amount.
    pub fn empty(
        validator_public_key: PublicKey,
        delegator_kind: DelegatorKind,
        bonding_purse: URef,
    ) -> Self {
        let vesting_schedule = None;
        let staked_amount = 0.into();
        Self {
            validator_public_key,
            delegator_kind,
            bonding_purse,
            staked_amount,
            vesting_schedule,
        }
    }

    /// Sets validator public key
    pub fn with_validator_public_key(&mut self, validator_public_key: PublicKey) -> &mut Self {
        self.validator_public_key = validator_public_key;
        self
    }

    /// BidAddr for this instance.
    pub fn bid_addr(&self) -> BidAddr {
        match &self.delegator_kind {
            DelegatorKind::PublicKey(pk) => BidAddr::DelegatedAccount {
                validator: self.validator_public_key.clone().to_account_hash(),
                delegator: pk.clone().to_account_hash(),
            },
            DelegatorKind::Purse(addr) => BidAddr::DelegatedPurse {
                validator: self.validator_public_key.clone().to_account_hash(),
                delegator: *addr,
            },
        }
    }

    /// BidAddr for this instance.
    pub fn unbond_kind(&self) -> UnbondKind {
        match &self.delegator_kind {
            DelegatorKind::PublicKey(pk) => UnbondKind::DelegatedPublicKey(pk.clone()),
            DelegatorKind::Purse(addr) => UnbondKind::DelegatedPurse(*addr),
        }
    }

    #[cfg(any(feature = "testing", test))]
    pub fn random_for_validator_and_delegator(
        rng: &mut TestRng,
        validator_public_key: PublicKey,
        delegator_kind: DelegatorKind,
    ) -> Self {
        Self {
            delegator_kind,
            staked_amount: rng.gen(),
            bonding_purse: rng.gen(),
            validator_public_key,
            vesting_schedule: rng.gen(),
        }
    }
}

impl CLTyped for DelegatorBid {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for DelegatorBid {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.delegator_kind.to_bytes()?);
        buffer.extend(self.staked_amount.to_bytes()?);
        buffer.extend(self.bonding_purse.to_bytes()?);
        buffer.extend(self.validator_public_key.to_bytes()?);
        buffer.extend(self.vesting_schedule.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.delegator_kind.serialized_length()
            + self.staked_amount.serialized_length()
            + self.bonding_purse.serialized_length()
            + self.validator_public_key.serialized_length()
            + self.vesting_schedule.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.delegator_kind.write_bytes(writer)?;
        self.staked_amount.write_bytes(writer)?;
        self.bonding_purse.write_bytes(writer)?;
        self.validator_public_key.write_bytes(writer)?;
        self.vesting_schedule.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for DelegatorBid {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (delegator_kind, bytes) = DelegatorKind::from_bytes(bytes)?;
        let (staked_amount, bytes) = U512::from_bytes(bytes)?;
        let (bonding_purse, bytes) = URef::from_bytes(bytes)?;
        let (validator_public_key, bytes) = PublicKey::from_bytes(bytes)?;
        let (vesting_schedule, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            DelegatorBid {
                delegator_kind,
                staked_amount,
                bonding_purse,
                validator_public_key,
                vesting_schedule,
            },
            bytes,
        ))
    }
}

impl Display for DelegatorBid {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "delegator {{ {} {} motes, bonding purse {}, validator {} }}",
            self.delegator_kind, self.staked_amount, self.bonding_purse, self.validator_public_key
        )
    }
}

impl From<Delegator> for DelegatorBid {
    fn from(value: Delegator) -> Self {
        DelegatorBid {
            delegator_kind: value.delegator_public_key().clone().into(),
            validator_public_key: value.validator_public_key().clone(),
            staked_amount: value.staked_amount(),
            bonding_purse: *value.bonding_purse(),
            vesting_schedule: value.vesting_schedule().cloned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        bytesrepr,
        system::auction::{delegator_kind::DelegatorKind, DelegatorBid},
        AccessRights, PublicKey, SecretKey, URef, U512,
    };

    #[test]
    fn serialization_roundtrip() {
        let staked_amount = U512::one();
        let bonding_purse = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
        let delegator_public_key: PublicKey = PublicKey::from(
            &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
        );

        let delegator_kind = DelegatorKind::PublicKey(delegator_public_key);

        let validator_public_key: PublicKey = PublicKey::from(
            &SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let unlocked_delegator = DelegatorBid::unlocked(
            delegator_kind.clone(),
            staked_amount,
            bonding_purse,
            validator_public_key.clone(),
        );
        bytesrepr::test_serialization_roundtrip(&unlocked_delegator);

        let release_timestamp_millis = 42;
        let locked_delegator = DelegatorBid::locked(
            delegator_kind,
            staked_amount,
            bonding_purse,
            validator_public_key,
            release_timestamp_millis,
        );
        bytesrepr::test_serialization_roundtrip(&locked_delegator);
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid(bid in gens::delegator_bid_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid);
        }
    }
}
