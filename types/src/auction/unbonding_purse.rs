use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    bytesrepr::{self, ToBytes},
    CLType, CLTyped, PublicKey, URef, U512,
};
use bytesrepr::FromBytes;

/// Unbonding purse.
#[cfg_attr(test, derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct UnbondingPurse {
    /// Unbonding Purse.
    pub purse: URef,
    /// Unbonding Origin.
    pub origin: PublicKey,
    /// Unbonding Era.
    pub era_of_withdrawal: u64,
    /// Unbonding Amount.
    pub amount: U512,
}

impl ToBytes for UnbondingPurse {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(&self.purse.to_bytes()?);
        result.extend(&self.origin.to_bytes()?);
        result.extend(&self.era_of_withdrawal.to_bytes()?);
        result.extend(&self.amount.to_bytes()?);
        Ok(result)
    }
    fn serialized_length(&self) -> usize {
        self.purse.serialized_length()
            + self.origin.serialized_length()
            + self.era_of_withdrawal.serialized_length()
            + self.amount.serialized_length()
    }
}

impl FromBytes for UnbondingPurse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (origin, bytes) = FromBytes::from_bytes(bytes)?;
        let (era_of_withdrawal, bytes) = FromBytes::from_bytes(bytes)?;
        let (amount, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            UnbondingPurse {
                purse,
                origin,
                era_of_withdrawal,
                amount,
            },
            bytes,
        ))
    }
}

impl CLTyped for UnbondingPurse {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

/// Validators and delegators mapped to their purses, validator/bidder key of origin, era of
/// withdrawal, tokens and expiration timer in eras.
pub type UnbondingPurses = BTreeMap<PublicKey, Vec<UnbondingPurse>>;

#[cfg(test)]
mod tests {
    use super::UnbondingPurse;
    use crate::{bytesrepr, AccessRights, PublicKey, URef, U512};

    #[test]
    fn serialization_roundtrip() {
        let public_key = PublicKey::Ed25519([42; 32]);
        let unbonding_purse = UnbondingPurse {
            purse: URef::new([42; 32], AccessRights::READ_ADD_WRITE),
            origin: public_key,
            era_of_withdrawal: u64::max_value(),
            amount: U512::max_value() - 1,
        };
        bytesrepr::test_serialization_roundtrip(&unbonding_purse);
    }
}
