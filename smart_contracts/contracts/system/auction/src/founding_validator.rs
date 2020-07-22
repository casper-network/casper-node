use alloc::{collections::BTreeMap, vec::Vec};

use types::{
    account::AccountHash,
    bytesrepr::{self, FromBytes},
    CLType, CLTyped, URef, U512,
};

use crate::DelegationRate;
use bytesrepr::ToBytes;

pub struct FoundingValidator {
    pub bonding_purse: URef,
    pub staked_amount: U512,
    pub delegation_rate: DelegationRate,
    pub winner: bool,
}

impl CLTyped for FoundingValidator {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for FoundingValidator {
    fn to_bytes(&self) -> Result<Vec<u8>, types::bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(self.bonding_purse.to_bytes()?);
        result.extend(self.staked_amount.to_bytes()?);
        result.extend(self.delegation_rate.to_bytes()?);
        result.extend(self.winner.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.bonding_purse.serialized_length()
            + self.staked_amount.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.winner.serialized_length()
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
                winner,
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
pub type FoundingValidators = BTreeMap<AccountHash, FoundingValidator>;
