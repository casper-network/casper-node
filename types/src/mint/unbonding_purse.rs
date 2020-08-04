use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    account::AccountHash,
    bytesrepr::{self, ToBytes},
    CLType, CLTyped, URef,
};
use bytesrepr::FromBytes;

#[derive(Copy, Clone)]
pub struct UnbondingPurse {
    pub purse: URef,
    pub origin: AccountHash,
    pub era_of_withdrawal: u16,
    pub expiration_timer: u8,
}

impl ToBytes for UnbondingPurse {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(&self.purse.to_bytes()?);
        result.extend(&self.origin.to_bytes()?);
        result.extend(&self.era_of_withdrawal.to_bytes()?);
        result.extend(&self.expiration_timer.to_bytes()?);
        Ok(result)
    }
    fn serialized_length(&self) -> usize {
        self.purse.serialized_length()
            + self.origin.serialized_length()
            + self.era_of_withdrawal.serialized_length()
            + self.expiration_timer.serialized_length()
    }
}

impl FromBytes for UnbondingPurse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (purse, bytes) = FromBytes::from_bytes(bytes)?;
        let (origin, bytes) = FromBytes::from_bytes(bytes)?;
        let (era_of_withdrawal, bytes) = FromBytes::from_bytes(bytes)?;
        let (expiration_timer, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            UnbondingPurse {
                purse,
                origin,
                era_of_withdrawal,
                expiration_timer,
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
pub type UnbondingPurses = BTreeMap<AccountHash, Vec<UnbondingPurse>>;
