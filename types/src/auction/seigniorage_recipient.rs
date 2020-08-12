use alloc::{collections::BTreeMap, vec::Vec};

use super::{types::DelegationRate, DelegatedAmounts};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey, U512,
};

/// The seigniorage recipient details.
#[cfg_attr(test, derive(Debug))]
#[derive(Default, PartialEq)]
pub struct SeigniorageRecipient {
    /// Total staked amounts
    pub stake: U512,
    /// Accumulated delegation rates.
    pub delegation_rate: DelegationRate,
    /// List of delegators and their accumulated bids.
    pub delegators: DelegatedAmounts,
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

/// Collection of seigniorage recipients.
pub type SeigniorageRecipients = BTreeMap<PublicKey, SeigniorageRecipient>;

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
