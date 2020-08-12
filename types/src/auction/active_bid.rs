use alloc::{collections::BTreeMap, vec::Vec};

use super::types::DelegationRate;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey, URef, U512,
};

#[cfg_attr(test, derive(Debug))]
#[derive(PartialEq)]
/// Contains definition of an active bid.
pub struct ActiveBid {
    /// A purse that was used to bid.
    pub bid_purse: URef,
    /// Total amount of bid.
    pub bid_amount: U512,
    /// Rate of delegation.
    pub delegation_rate: DelegationRate,
}

impl CLTyped for ActiveBid {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for ActiveBid {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(self.bid_purse.to_bytes()?);
        result.extend(self.bid_amount.to_bytes()?);
        result.extend(self.delegation_rate.to_bytes()?);
        Ok(result)
    }
    fn serialized_length(&self) -> usize {
        self.bid_purse.serialized_length()
            + self.bid_amount.serialized_length()
            + self.delegation_rate.serialized_length()
    }
}

impl FromBytes for ActiveBid {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bid_purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (bid_amount, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            ActiveBid {
                bid_purse,
                bid_amount,
                delegation_rate,
            },
            bytes,
        ))
    }
}

/// Validators, mapped to their their purses, bids (active or bonded) and
/// rates. There is no distinction in behavior between the bid of an active
/// validator and a prospective validator - reducing the bid results in the
/// tokens being transferred to an unbonding purse either way.
pub type ActiveBids = BTreeMap<PublicKey, ActiveBid>;

#[cfg(test)]
mod tests {
    use super::ActiveBid;
    use crate::{auction::DelegationRate, bytesrepr, AccessRights, URef, U512};

    #[test]
    fn serialization_roundtrip() {
        let active_bid = ActiveBid {
            bid_purse: URef::new([42; 32], AccessRights::READ_ADD_WRITE),
            bid_amount: U512::max_value(),
            delegation_rate: DelegationRate::max_value(),
        };
        bytesrepr::test_serialization_roundtrip(&active_bid);
    }
}
