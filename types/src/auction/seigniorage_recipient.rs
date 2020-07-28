use alloc::{collections::BTreeMap, vec::Vec};

use super::types::DelegationRate;
use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, U512,
};

/// The seigniorage recipient details.
#[cfg_attr(test, derive(Debug))]
#[derive(PartialEq)]
pub struct SeigniorageRecipient {
    stake: U512,
    delegation_rate: DelegationRate,
    delegators: Vec<(AccountHash, U512)>,
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
pub type SeigniorageRecipients = BTreeMap<AccountHash, SeigniorageRecipient>;

#[cfg(test)]
mod tests {
    use super::SeigniorageRecipient;
    use crate::{account::AccountHash, auction::DelegationRate, bytesrepr, U512};

    #[test]
    fn serialization_roundtrip() {
        let seigniorage_recipient = SeigniorageRecipient {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::max_value(),
            delegators: vec![
                (AccountHash::new([42; 32]), U512::one()),
                (AccountHash::new([43; 32]), U512::max_value()),
                (AccountHash::new([44; 32]), U512::zero()),
            ],
        };
        bytesrepr::test_serialization_roundtrip(&seigniorage_recipient);
    }
}
