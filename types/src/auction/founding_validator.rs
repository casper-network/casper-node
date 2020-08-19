use alloc::{collections::BTreeMap, vec::Vec};

use super::types::DelegationRate;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey, URef, U512,
};

/// An entry in a founding validator map.
#[derive(PartialEq, Debug)]
pub struct Validator {
    /// The purse that was used for bonding.
    pub bonding_purse: URef,
    /// The total amount of staked tokens.
    pub staked_amount: U512,
    /// Delegation rate
    pub delegation_rate: DelegationRate,
    /// A flag that represents a winning entry.
    pub funds_locked: bool,
    /// A flag that indicates if given entry represents a founding validator.
    pub is_founding_validator: bool,
}

impl Validator {
    /// Creates new instance of `FoundingValidator`.
    pub fn new_founding_validator(bonding_purse: URef, staked_amount: U512) -> Self {
        Self {
            bonding_purse,
            staked_amount,
            delegation_rate: 0,
            funds_locked: true,
            is_founding_validator: true,
        }
    }

    /// Creates a new instance of `FoundingValidator` for non-founding entry.
    pub fn new(bonding_purse: URef, staked_amount: U512) -> Self {
        Self {
            bonding_purse,
            staked_amount,
            delegation_rate: 0,
            funds_locked: true,
            is_founding_validator: true,
        }
    }

    /// Checks if a given founding validator can release its funds.
    pub fn can_release_funds(&self) -> bool {
        self.is_founding_validator && self.funds_locked
    }

    /// Checks if a given founding validator can release its funds.
    pub fn can_withdraw_funds(&self) -> bool {
        if self.is_founding_validator {
            // Only unlocked funds
            !self.funds_locked
        } else {
            // Non founding validator can always withdraw funds
            true
        }
    }
}

impl CLTyped for Validator {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for Validator {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(self.bonding_purse.to_bytes()?);
        result.extend(self.staked_amount.to_bytes()?);
        result.extend(self.delegation_rate.to_bytes()?);
        result.extend(self.funds_locked.to_bytes()?);
        result.extend(self.is_founding_validator.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.bonding_purse.serialized_length()
            + self.staked_amount.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.funds_locked.serialized_length()
            + self.is_founding_validator.serialized_length()
    }
}

impl FromBytes for Validator {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bonding_purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (staked_amount, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (funds_locked, bytes) = FromBytes::from_bytes(bytes)?;
        let (is_founding_validator, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            Validator {
                bonding_purse,
                staked_amount,
                delegation_rate,
                funds_locked,
                is_founding_validator,
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
///
/// This structure also contains bids, and founding validator and a bid is
/// differentiated by the `is_founding_validator` attribute.
pub type Validators = BTreeMap<PublicKey, Validator>;

#[cfg(test)]
mod tests {
    use super::Validator;
    use crate::{auction::DelegationRate, bytesrepr, AccessRights, URef, U512};

    #[test]
    fn serialization_roundtrip() {
        let founding_validator = Validator {
            bonding_purse: URef::new([42; 32], AccessRights::READ_ADD_WRITE),
            staked_amount: U512::one(),
            delegation_rate: DelegationRate::max_value(),
            funds_locked: true,
            is_founding_validator: false,
        };
        bytesrepr::test_serialization_roundtrip(&founding_validator);
    }
}
