use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    auction::{Bid, DelegationRate, Delegator},
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey, U512,
};

/// The seigniorage recipient details.
#[cfg_attr(test, derive(Debug))]
#[derive(Default, PartialEq, Clone)]
pub struct SeigniorageRecipient {
    /// Validator stake (not including delegators)
    pub stake: U512,
    /// Delegation rate of a seigniorage recipient.
    pub delegation_rate: DelegationRate,
    /// List of delegators and their accumulated bids.
    pub delegators: BTreeMap<PublicKey, Delegator>,
}

impl SeigniorageRecipient {
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
    fn from(founding_validator: &Bid) -> Self {
        Self {
            stake: *founding_validator.staked_amount(),
            delegation_rate: *founding_validator.delegation_rate(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;
    use core::iter::FromIterator;

    use crate::{
        auction::{DelegationRate, Delegator, SeigniorageRecipient},
        bytesrepr, AccessRights, PublicKey, URef, U512,
    };

    #[test]
    fn serialization_roundtrip() {
        let uref = URef::new([0; 32], AccessRights::READ_ADD_WRITE);
        let seigniorage_recipient = SeigniorageRecipient {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::max_value(),
            delegators: BTreeMap::from_iter(vec![
                (
                    PublicKey::Ed25519([1; 32]),
                    Delegator::new(U512::max_value(), uref, PublicKey::Ed25519([42; 32])),
                ),
                (
                    PublicKey::Ed25519([1; 32]),
                    Delegator::new(U512::max_value(), uref, PublicKey::Ed25519([43; 32])),
                ),
                (
                    PublicKey::Ed25519([1; 32]),
                    Delegator::new(U512::zero(), uref, PublicKey::Ed25519([44; 32])),
                ),
            ]),
        };
        bytesrepr::test_serialization_roundtrip(&seigniorage_recipient);
    }
}
