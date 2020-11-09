use alloc::vec::Vec;

use crate::{
    auction::{Bid, DelegatedAmounts, DelegationRate},
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, U512,
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
    pub delegators: DelegatedAmounts,
}

impl SeigniorageRecipient {
    /// Calculates total stake, including delegators' total stake
    pub fn total_stake(&self) -> U512 {
        self.stake + self.delegator_total_stake()
    }

    /// Caculates total stake for all delegators
    pub fn delegator_total_stake(&self) -> U512 {
        self.delegators.values().cloned().sum()
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

    use super::SeigniorageRecipient;
    use crate::{auction::DelegationRate, bytesrepr, PublicKey, U512};

    #[test]
    fn serialization_roundtrip() {
        let seigniorage_recipient = SeigniorageRecipient {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::max_value(),
            delegators: BTreeMap::from_iter(vec![
                (PublicKey::Ed25519([42; 32]), U512::one()),
                (PublicKey::Ed25519([43; 32]), U512::max_value()),
                (PublicKey::Ed25519([44; 32]), U512::zero()),
            ]),
        };
        bytesrepr::test_serialization_roundtrip(&seigniorage_recipient);
    }
}
