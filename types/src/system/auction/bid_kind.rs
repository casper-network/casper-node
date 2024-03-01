use crate::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    system::auction::{bid::VestingSchedule, Bid, Delegator, ValidatorBid},
    PublicKey, URef, U512,
};

use crate::system::auction::BidAddr;
use alloc::{boxed::Box, vec::Vec};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// BidKindTag variants.
#[allow(clippy::large_enum_variant)]
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum BidKindTag {
    /// Unified bid.
    Unified = 0,
    /// Validator bid.
    Validator = 1,
    /// Delegator bid.
    Delegator = 2,
}

/// Auction bid variants.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BidKind {
    /// A unified record indexed on validator data, with an embedded collection of all delegator
    /// bids assigned to that validator. The Unified variant is for legacy retrograde support, new
    /// instances will not be created going forward.
    Unified(Box<Bid>),
    /// A bid record containing only validator data.
    Validator(Box<ValidatorBid>),
    /// A bid record containing only delegator data.
    Delegator(Box<Delegator>),
}

impl BidKind {
    /// Returns validator public key.
    pub fn validator_public_key(&self) -> PublicKey {
        match self {
            BidKind::Unified(bid) => bid.validator_public_key().clone(),
            BidKind::Validator(validator_bid) => validator_bid.validator_public_key().clone(),
            BidKind::Delegator(delegator_bid) => delegator_bid.validator_public_key().clone(),
        }
    }

    /// Returns delegator public key, if any.
    pub fn maybe_delegator_public_key(&self) -> Option<PublicKey> {
        match self {
            BidKind::Unified(_) | BidKind::Validator(_) => None,
            BidKind::Delegator(delegator_bid) => Some(delegator_bid.delegator_public_key().clone()),
        }
    }

    /// Returns BidAddr.
    pub fn bid_addr(&self) -> BidAddr {
        match self {
            BidKind::Unified(bid) => BidAddr::Unified(bid.validator_public_key().to_account_hash()),
            BidKind::Validator(validator_bid) => {
                BidAddr::Validator(validator_bid.validator_public_key().to_account_hash())
            }
            BidKind::Delegator(delegator_bid) => {
                let validator = delegator_bid.validator_public_key().to_account_hash();
                let delegator = delegator_bid.delegator_public_key().to_account_hash();
                BidAddr::Delegator {
                    validator,
                    delegator,
                }
            }
        }
    }

    /// Is this instance a unified bid?.
    pub fn is_unified(&self) -> bool {
        match self {
            BidKind::Unified(_) => true,
            BidKind::Validator(_) | BidKind::Delegator(_) => false,
        }
    }

    /// Is this instance a validator bid?.
    pub fn is_validator(&self) -> bool {
        match self {
            BidKind::Validator(_) => true,
            BidKind::Unified(_) | BidKind::Delegator(_) => false,
        }
    }

    /// Is this instance a delegator bid?.
    pub fn is_delegator(&self) -> bool {
        match self {
            BidKind::Delegator(_) => true,
            BidKind::Unified(_) | BidKind::Validator(_) => false,
        }
    }

    /// The staked amount.
    pub fn staked_amount(&self) -> U512 {
        match self {
            BidKind::Unified(bid) => *bid.staked_amount(),
            BidKind::Validator(validator_bid) => validator_bid.staked_amount(),
            BidKind::Delegator(delegator) => delegator.staked_amount(),
        }
    }

    /// The bonding purse.
    pub fn bonding_purse(&self) -> URef {
        match self {
            BidKind::Unified(bid) => *bid.bonding_purse(),
            BidKind::Validator(validator_bid) => *validator_bid.bonding_purse(),
            BidKind::Delegator(delegator) => *delegator.bonding_purse(),
        }
    }

    /// The delegator public key, if relevant.
    pub fn delegator_public_key(&self) -> Option<PublicKey> {
        match self {
            BidKind::Unified(_) | BidKind::Validator(_) => None,
            BidKind::Delegator(delegator) => Some(delegator.delegator_public_key().clone()),
        }
    }

    /// Is this bid inactive?
    pub fn inactive(&self) -> bool {
        match self {
            BidKind::Unified(bid) => bid.inactive(),
            BidKind::Validator(validator_bid) => validator_bid.inactive(),
            BidKind::Delegator(delegator) => delegator.staked_amount().is_zero(),
        }
    }

    /// Checks if a bid is still locked under a vesting schedule.
    ///
    /// Returns true if a timestamp falls below the initial lockup period + 91 days release
    /// schedule, otherwise false.
    pub fn is_locked(&self, timestamp_millis: u64) -> bool {
        match self {
            BidKind::Unified(bid) => bid.is_locked(timestamp_millis),
            BidKind::Validator(validator_bid) => validator_bid.is_locked(timestamp_millis),
            BidKind::Delegator(delegator) => delegator.is_locked(timestamp_millis),
        }
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
        match self {
            BidKind::Unified(bid) => bid
                .is_locked_with_vesting_schedule(timestamp_millis, vesting_schedule_period_millis),
            BidKind::Validator(validator_bid) => validator_bid
                .is_locked_with_vesting_schedule(timestamp_millis, vesting_schedule_period_millis),
            BidKind::Delegator(delegator) => delegator
                .is_locked_with_vesting_schedule(timestamp_millis, vesting_schedule_period_millis),
        }
    }

    /// Returns a reference to the vesting schedule of the provided bid.  `None` if a non-genesis
    /// validator.
    pub fn vesting_schedule(&self) -> Option<&VestingSchedule> {
        match self {
            BidKind::Unified(bid) => bid.vesting_schedule(),
            BidKind::Validator(validator_bid) => validator_bid.vesting_schedule(),
            BidKind::Delegator(delegator) => delegator.vesting_schedule(),
        }
    }

    /// BidKindTag.
    pub fn tag(&self) -> BidKindTag {
        match self {
            BidKind::Unified(_) => BidKindTag::Unified,
            BidKind::Validator(_) => BidKindTag::Validator,
            BidKind::Delegator(_) => BidKindTag::Delegator,
        }
    }
}

impl ToBytes for BidKind {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        let (tag, mut serialized_data) = match self {
            BidKind::Unified(bid) => (BidKindTag::Unified, bid.to_bytes()?),
            BidKind::Validator(validator_bid) => (BidKindTag::Validator, validator_bid.to_bytes()?),
            BidKind::Delegator(delegator_bid) => (BidKindTag::Delegator, delegator_bid.to_bytes()?),
        };
        result.push(tag as u8);
        result.append(&mut serialized_data);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                BidKind::Unified(bid) => bid.serialized_length(),
                BidKind::Validator(validator_bid) => validator_bid.serialized_length(),
                BidKind::Delegator(delegator_bid) => delegator_bid.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag() as u8);
        match self {
            //StoredValue::CLValue(cl_value) => cl_value.write_bytes(writer)?,
            BidKind::Unified(bid) => bid.write_bytes(writer)?,
            BidKind::Validator(validator_bid) => validator_bid.write_bytes(writer)?,
            BidKind::Delegator(delegator_bid) => delegator_bid.write_bytes(writer)?,
        };
        Ok(())
    }
}

impl FromBytes for BidKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            tag if tag == BidKindTag::Unified as u8 => Bid::from_bytes(remainder)
                .map(|(bid, remainder)| (BidKind::Unified(Box::new(bid)), remainder)),
            tag if tag == BidKindTag::Validator as u8 => {
                ValidatorBid::from_bytes(remainder).map(|(validator_bid, remainder)| {
                    (BidKind::Validator(Box::new(validator_bid)), remainder)
                })
            }
            tag if tag == BidKindTag::Delegator as u8 => {
                Delegator::from_bytes(remainder).map(|(delegator_bid, remainder)| {
                    (BidKind::Delegator(Box::new(delegator_bid)), remainder)
                })
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BidKind, *};
    use crate::{bytesrepr, system::auction::DelegationRate, AccessRights, SecretKey};

    #[test]
    fn serialization_roundtrip() {
        let validator_public_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([0u8; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let bonding_purse = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
        let bid = Bid::unlocked(
            validator_public_key.clone(),
            bonding_purse,
            U512::one(),
            DelegationRate::max_value(),
        );
        let unified_bid = BidKind::Unified(Box::new(bid.clone()));
        let validator_bid = ValidatorBid::from(bid.clone());

        let delegator_public_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([1u8; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator = Delegator::unlocked(
            delegator_public_key,
            U512::one(),
            bonding_purse,
            validator_public_key,
        );
        let delegator_bid = BidKind::Delegator(Box::new(delegator));

        bytesrepr::test_serialization_roundtrip(&bid);
        bytesrepr::test_serialization_roundtrip(&unified_bid);
        bytesrepr::test_serialization_roundtrip(&validator_bid);
        bytesrepr::test_serialization_roundtrip(&delegator_bid);
    }
}

#[cfg(test)]
mod prop_test_bid_kind_unified {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid_kind_unified(bid_kind in gens::unified_bid_arb(0..3)) {
            bytesrepr::test_serialization_roundtrip(&bid_kind);
        }
    }
}

#[cfg(test)]
mod prop_test_bid_kind_validator {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid_kind_validator(bid_kind in gens::validator_bid_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid_kind);
        }
    }
}

#[cfg(test)]
mod prop_test_bid_kind_delegator {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid_kind_delegator(bid_kind in gens::delegator_bid_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid_kind);
        }
    }
}
