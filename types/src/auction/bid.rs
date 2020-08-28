use alloc::{collections::BTreeMap, vec::Vec};

use super::{types::DelegationRate, EraId};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey, URef, U512,
};

/// An entry in a founding validator map.
#[derive(PartialEq, Debug)]
pub struct Bid {
    /// The purse that was used for bonding.
    pub bonding_purse: URef,
    /// The total amount of staked tokens.
    pub staked_amount: U512,
    /// Delegation rate
    pub delegation_rate: DelegationRate,
    /// A flag that represents a winning entry.
    ///
    /// `Some` indicates locked funds for a specific era and an autowin status, and `None` case
    /// means that funds are unlocked and autowin status is removed.
    pub funds_locked: Option<EraId>,
}

impl Bid {
    /// Creates new instance of a bid with locked funds.
    pub fn new_locked(bonding_purse: URef, staked_amount: U512, funds_locked: EraId) -> Self {
        Self {
            bonding_purse,
            staked_amount,
            delegation_rate: 0,
            funds_locked: Some(funds_locked),
        }
    }

    /// Checks if a given founding validator can release its funds.
    pub fn can_release_funds(&self) -> bool {
        self.funds_locked.is_some()
    }

    /// Checks if a given founding validator can withdraw its funds.
    pub fn can_withdraw_funds(&self) -> bool {
        self.funds_locked.is_none()
    }
}

impl CLTyped for Bid {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for Bid {
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

impl FromBytes for Bid {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bonding_purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (staked_amount, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (locked_until, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            Bid {
                bonding_purse,
                staked_amount,
                delegation_rate,
                funds_locked: locked_until,
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
pub type Bids = BTreeMap<PublicKey, Bid>;

#[cfg(test)]
mod tests {
    use super::Bid;
    use crate::{
        auction::{DelegationRate, EraId},
        bytesrepr, AccessRights, URef, U512,
    };

    #[test]
    fn serialization_roundtrip() {
        let founding_validator = Bid {
            bonding_purse: URef::new([42; 32], AccessRights::READ_ADD_WRITE),
            staked_amount: U512::one(),
            delegation_rate: DelegationRate::max_value(),
            funds_locked: Some(EraId::max_value() - 1),
        };
        bytesrepr::test_serialization_roundtrip(&founding_validator);
    }
}
