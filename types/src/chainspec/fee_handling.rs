#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

const FEE_HANDLING_PROPOSER_TAG: u8 = 0;
const FEE_HANDLING_ACCUMULATE_TAG: u8 = 1;
const FEE_HANDLING_BURN_TAG: u8 = 2;
const FEE_HANDLING_NONE_TAG: u8 = 3;

/// Defines how fees are handled in the system.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum FeeHandling {
    /// Transaction fees are paid to the block proposer.
    ///
    /// This is the default option for public chains.
    PayToProposer,
    /// Transaction fees are accumulated in a special purse and then distributed during end of era
    /// processing evenly among all administrator accounts.
    ///
    /// This setting is applicable for some private chains (but not all).
    Accumulate,
    /// Burn the fees.
    Burn,
    /// No fees.
    NoFee,
}

impl FeeHandling {
    /// Is the Accumulate variant selected?
    pub fn is_accumulate(&self) -> bool {
        matches!(self, FeeHandling::Accumulate)
    }

    /// Returns true if configured for no fees.
    pub fn is_no_fee(&self) -> bool {
        matches!(self, FeeHandling::NoFee)
    }
}

impl ToBytes for FeeHandling {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            FeeHandling::PayToProposer => Ok(vec![FEE_HANDLING_PROPOSER_TAG]),
            FeeHandling::Accumulate => Ok(vec![FEE_HANDLING_ACCUMULATE_TAG]),
            FeeHandling::Burn => Ok(vec![FEE_HANDLING_BURN_TAG]),
            FeeHandling::NoFee => Ok(vec![FEE_HANDLING_NONE_TAG]),
        }
    }

    fn serialized_length(&self) -> usize {
        1
    }
}

impl FromBytes for FeeHandling {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            FEE_HANDLING_PROPOSER_TAG => Ok((FeeHandling::PayToProposer, rem)),
            FEE_HANDLING_ACCUMULATE_TAG => Ok((FeeHandling::Accumulate, rem)),
            FEE_HANDLING_BURN_TAG => Ok((FeeHandling::Burn, rem)),
            FEE_HANDLING_NONE_TAG => Ok((FeeHandling::NoFee, rem)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Default for FeeHandling {
    fn default() -> Self {
        // in 1.x the (implicit) default was PayToProposer
        // FeeHandling::PayToProposer
        // in 2.x the default is NoFee as there are no fees.
        FeeHandling::NoFee
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip_for_refund() {
        let fee_config = FeeHandling::PayToProposer;
        bytesrepr::test_serialization_roundtrip(&fee_config);
    }

    #[test]
    fn bytesrepr_roundtrip_for_accumulate() {
        let fee_config = FeeHandling::Accumulate;
        bytesrepr::test_serialization_roundtrip(&fee_config);
    }

    #[test]
    fn bytesrepr_roundtrip_for_burn() {
        let fee_config = FeeHandling::Burn;
        bytesrepr::test_serialization_roundtrip(&fee_config);
    }

    #[test]
    fn bytesrepr_roundtrip_for_no_fee() {
        let fee_config = FeeHandling::NoFee;
        bytesrepr::test_serialization_roundtrip(&fee_config);
    }
}
