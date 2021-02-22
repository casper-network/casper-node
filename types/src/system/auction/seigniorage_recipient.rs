use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::{Bid, DelegationRate, Delegator},
    CLType, CLTyped, PublicKey, U512,
};

/// The seigniorage recipient details.
#[derive(Default, PartialEq, Clone, Debug)]
pub struct SeigniorageRecipient {
    /// Validator stake (not including delegators)
    stake: U512,
    /// Delegation rate of a seigniorage recipient.
    delegation_rate: DelegationRate,
    /// List of delegators and their accumulated bids.
    delegators: BTreeMap<PublicKey, Delegator>,
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

    /// Returns delegators of the provided recipient
    pub fn delegators(&self) -> &BTreeMap<PublicKey, Delegator> {
        &self.delegators
    }

    /// Calculates total stake, including delegators' total stake
    pub fn total_stake(&self) -> U512 {
        self.stake + self.delegator_total_stake()
    }

    /// Caculates total stake for all delegators
    pub fn delegator_total_stake(&self) -> U512 {
        self.delegators
            .values()
            .map(Delegator::staked_amount)
            .cloned()
            .sum()
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
        result.extend(self.delegators.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.stake.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.delegators.serialized_length()
    }
}

impl FromBytes for SeigniorageRecipient {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegators, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            SeigniorageRecipient {
                stake,
                delegation_rate,
                delegators,
            },
            bytes,
        ))
    }
}

impl From<&Bid> for SeigniorageRecipient {
    fn from(bid: &Bid) -> Self {
        Self {
            stake: *bid.staked_amount(),
            delegation_rate: *bid.delegation_rate(),
            delegators: bid.delegators().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;
    use core::iter::FromIterator;

    use crate::{
        bytesrepr,
        system::auction::{DelegationRate, Delegator, SeigniorageRecipient},
        AccessRights, SecretKey, URef, U512,
    };

    #[test]
    fn serialization_roundtrip() {
        let uref = URef::new([0; 32], AccessRights::READ_ADD_WRITE);
        let validator_key = SecretKey::ed25519([1; SecretKey::ED25519_LENGTH]).into();
        let delegator_1_key = SecretKey::ed25519([42; SecretKey::ED25519_LENGTH]).into();
        let delegator_2_key = SecretKey::ed25519([43; SecretKey::ED25519_LENGTH]).into();
        let delegator_3_key = SecretKey::ed25519([44; SecretKey::ED25519_LENGTH]).into();

        let seigniorage_recipient = SeigniorageRecipient {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::max_value(),
            delegators: BTreeMap::from_iter(vec![
                (
                    validator_key,
                    Delegator::new(delegator_1_key, U512::max_value(), uref, validator_key),
                ),
                (
                    validator_key,
                    Delegator::new(delegator_2_key, U512::max_value(), uref, validator_key),
                ),
                (
                    validator_key,
                    Delegator::new(delegator_3_key, U512::zero(), uref, validator_key),
                ),
            ]),
        };
        bytesrepr::test_serialization_roundtrip(&seigniorage_recipient);
    }
}
