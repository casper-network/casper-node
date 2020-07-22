use alloc::vec::Vec;

use types::{
    account::AccountHash,
    bytesrepr::{self, ToBytes},
    CLType, CLTyped, U512,
};

use crate::DelegationRate;
use bytesrepr::FromBytes;

pub struct SeignorageRecipient {
    stake: U512,
    delegation_rate: DelegationRate,
    delegators: Vec<(AccountHash, U512)>,
}

impl CLTyped for SeignorageRecipient {
    fn cl_type() -> types::CLType {
        CLType::Any
    }
}

impl ToBytes for SeignorageRecipient {
    fn to_bytes(&self) -> Result<Vec<u8>, types::bytesrepr::Error> {
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

impl FromBytes for SeignorageRecipient {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegators, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            SeignorageRecipient {
                stake,
                delegation_rate,
                delegators,
            },
            bytes,
        ))
    }
}
