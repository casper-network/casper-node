use alloc::vec::Vec;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey, URef, U512,
};

/// Unbonding purse.
#[cfg_attr(test, derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct UnbondingPurse {
    /// Bonding Purse
    pub bonding_purse: URef,
    /// Unbonding Purse.
    pub unbonding_purse: URef,
    /// Unbonding Origin.
    pub public_key: PublicKey,
    /// Unbonding Era.
    pub era_of_withdrawal: u64,
    /// Unbonding Amount.
    pub amount: U512,
}

impl ToBytes for UnbondingPurse {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(&self.bonding_purse.to_bytes()?);
        result.extend(&self.unbonding_purse.to_bytes()?);
        result.extend(&self.public_key.to_bytes()?);
        result.extend(&self.era_of_withdrawal.to_bytes()?);
        result.extend(&self.amount.to_bytes()?);
        Ok(result)
    }
    fn serialized_length(&self) -> usize {
        self.bonding_purse.serialized_length()
            + self.unbonding_purse.serialized_length()
            + self.public_key.serialized_length()
            + self.era_of_withdrawal.serialized_length()
            + self.amount.serialized_length()
    }
}

impl FromBytes for UnbondingPurse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bonding_purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (unbonding_purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (public_key, bytes) = FromBytes::from_bytes(bytes)?;
        let (era_of_withdrawal, bytes) = FromBytes::from_bytes(bytes)?;
        let (amount, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            UnbondingPurse {
                bonding_purse,
                unbonding_purse,
                public_key,
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

#[cfg(test)]
mod tests {
    use crate::{auction::UnbondingPurse, bytesrepr, AccessRights, PublicKey, URef, U512};

    #[test]
    fn serialization_roundtrip() {
        let unbonding_purse = UnbondingPurse {
            bonding_purse: URef::new([41; 32], AccessRights::READ_ADD_WRITE),
            unbonding_purse: URef::new([42; 32], AccessRights::READ_ADD_WRITE),
            public_key: PublicKey::Ed25519([42; 32]),
            era_of_withdrawal: u64::max_value(),
            amount: U512::max_value() - 1,
        };
        bytesrepr::test_serialization_roundtrip(&unbonding_purse);
    }
}
