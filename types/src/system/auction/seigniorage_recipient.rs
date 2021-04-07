// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::{collections::BTreeMap, vec::Vec};

#[cfg(feature = "std")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::{Bid, DelegationRate},
    CLType, CLTyped, PublicKey, U512,
};

/// The seigniorage recipient details.
#[derive(Default, PartialEq, Clone, Debug, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "std", derive(JsonSchema))]
pub struct SeigniorageRecipient {
    /// Validator stake (not including delegators)
    stake: U512,
    /// Delegation rate of a seigniorage recipient.
    delegation_rate: DelegationRate,
    /// Delegators and their bids.
    delegator_stake: BTreeMap<PublicKey, U512>,
}

impl SeigniorageRecipient {
    /// Returns stake of the provided recipient
    pub fn stake(&self) -> &U512 {
        &self.stake
    }

    /// Returns delegation rate of the provided recipient
    pub fn delegation_rate(&self) -> &DelegationRate {
        &self.delegation_rate
    }

    /// Returns delegators of the provided recipient and their stake
    pub fn delegator_stake(&self) -> &BTreeMap<PublicKey, U512> {
        &self.delegator_stake
    }

    /// Calculates total stake, including delegators' total stake
    pub fn total_stake(&self) -> Option<U512> {
        match self.delegator_total_stake() {
            Some(stake) => self.stake.checked_add(stake),
            None => None,
        }
    }

    /// Calculates total stake for all delegators
    pub fn delegator_total_stake(&self) -> Option<U512> {
        let mut total_stake: U512 = U512::zero();
        for (_, stake) in self.delegator_stake.iter() {
            total_stake = match total_stake.checked_add(*stake) {
                Some(stake) => stake,
                None => return None,
            }
        }
        Some(total_stake)
    }
}

impl CLTyped for SeigniorageRecipient {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for SeigniorageRecipient {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(self.stake.to_bytes()?);
        result.extend(self.delegation_rate.to_bytes()?);
        result.extend(self.delegator_stake.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.stake.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.delegator_stake.serialized_length()
    }
}

impl FromBytes for SeigniorageRecipient {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegator_stake, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            SeigniorageRecipient {
                stake,
                delegation_rate,
                delegator_stake,
            },
            bytes,
        ))
    }
}

impl From<&Bid> for SeigniorageRecipient {
    fn from(bid: &Bid) -> Self {
        let delegator_stake = bid
            .delegators()
            .iter()
            .map(|(public_key, delegator)| (*public_key, *delegator.staked_amount()))
            .collect();
        Self {
            stake: *bid.staked_amount(),
            delegation_rate: *bid.delegation_rate(),
            delegator_stake,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;
    use core::iter::FromIterator;

    use crate::{
        bytesrepr,
        system::auction::{DelegationRate, SeigniorageRecipient},
        SecretKey, U512,
    };

    #[test]
    fn serialization_roundtrip() {
        let delegator_1_key = SecretKey::ed25519([42; SecretKey::ED25519_LENGTH]).into();
        let delegator_2_key = SecretKey::ed25519([43; SecretKey::ED25519_LENGTH]).into();
        let delegator_3_key = SecretKey::ed25519([44; SecretKey::ED25519_LENGTH]).into();
        let seigniorage_recipient = SeigniorageRecipient {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::max_value(),
            delegator_stake: BTreeMap::from_iter(vec![
                (delegator_1_key, U512::max_value()),
                (delegator_2_key, U512::max_value()),
                (delegator_3_key, U512::zero()),
            ]),
        };
        bytesrepr::test_serialization_roundtrip(&seigniorage_recipient);
    }

    #[test]
    fn test_overflow_in_delegation_rate() {
        let delegator_1_key = SecretKey::ed25519([42; SecretKey::ED25519_LENGTH]).into();
        let delegator_2_key = SecretKey::ed25519([43; SecretKey::ED25519_LENGTH]).into();
        let delegator_3_key = SecretKey::ed25519([44; SecretKey::ED25519_LENGTH]).into();
        let seigniorage_recipient = SeigniorageRecipient {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::max_value(),
            delegator_stake: BTreeMap::from_iter(vec![
                (delegator_1_key, U512::max_value()),
                (delegator_2_key, U512::max_value()),
                (delegator_3_key, U512::zero()),
            ]),
        };
        assert_eq!(seigniorage_recipient.total_stake(), None)
    }
}
