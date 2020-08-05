use alloc::{collections::BTreeMap, vec::Vec};

use super::types::DelegationRate;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey, URef, U512,
};

use super::types::CommissionRate;

/// An entry in a founding validator map.
#[derive(PartialEq, Debug)]
pub struct FoundingValidator {
    /// The purse that was used for bonding.
    pub bonding_purse: URef,
    /// The total amount of staked tokens.
    pub staked_amount: U512,
    /// Commission rate
    pub commission_rate: CommissionRate,
    /// A flag that represents a winning entry.
    pub funds_locked: bool,
}

impl FoundingValidator {
    /// Creates new instance of `FoundingValidator`.
    pub fn new(bonding_purse: URef, staked_amount: U512) -> Self {
        Self {
            bonding_purse,
            staked_amount,
            commission_rate: 0,
            funds_locked: true,
        }
    }
}

impl CLTyped for FoundingValidator {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for FoundingValidator {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(self.bonding_purse.to_bytes()?);
        result.extend(self.staked_amount.to_bytes()?);
        result.extend(self.delegation_rate.to_bytes()?);
        result.extend(self.funds_locked.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.bonding_purse.serialized_length()
            + self.staked_amount.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.funds_locked.serialized_length()
    }
}

impl FromBytes for FoundingValidator {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bonding_purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (staked_amount, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (winner, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            FoundingValidator {
                bonding_purse,
                staked_amount,
                delegation_rate,
                funds_locked: winner,
            },
            bytes,
        ))
    }
}

/// Founding validators' public keys mapped to their staked
/// amount, bid purse held by the mint contract, delegation rate and
/// whether they are to be considered for the auction, or automatically
/// entered as “winners” (this also locks them out of unbonding), taking
/// some slots out of the auction. The autowin status is controlled by
/// node software and would, presumably, expire after a fixed number of eras.
pub type FoundingValidators = BTreeMap<PublicKey, FoundingValidator>;

#[cfg(test)]
mod tests {
    use super::FoundingValidator;
    use crate::{auction::CommissionRate, bytesrepr, AccessRights, URef, U512};

    #[test]
    fn serialization_roundtrip() {
        let founding_validator = FoundingValidator {
            bonding_purse: URef::new([42; 32], AccessRights::READ_ADD_WRITE),
            staked_amount: U512::one(),
            commission_rate: CommissionRate::max_value(),
            funds_locked: true,
        };
        bytesrepr::test_serialization_roundtrip(&founding_validator);
    }
}
