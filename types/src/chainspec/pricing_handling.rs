use crate::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
};
use core::fmt::{Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

const PRICING_HANDLING_TAG_LENGTH: u8 = 1;

const PRICING_HANDLING_CLASSIC_TAG: u8 = 0;
const PRICING_HANDLING_FIXED_TAG: u8 = 1;
const PRICING_HANDLING_GASLIMITED_TAG: u8 = 2;

/// Defines what pricing mode a network allows. Correlates to the PricingMode of a
/// [`crate::Transaction`]. Nodes will not accept transactions whose pricing mode does not match.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum PricingHandling {
    /// The transaction sender self-specifies how much token they pay, which becomes their gas
    /// limit.
    Classic,
    /// The costs are fixed, per the cost tables.
    // in 2.0 the default pricing handling is Fixed
    #[default]
    Fixed,
    /// The transaction sender self-specifies how much token they want to use as a gas limit, which
    /// becomes their gas.
    GasLimited,
}

impl Display for PricingHandling {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            PricingHandling::Classic => {
                write!(f, "PricingHandling::Classic")
            }
            PricingHandling::Fixed => {
                write!(f, "PricingHandling::Fixed")
            }
            PricingHandling::GasLimited => {
                write!(f, "PricingHandling::GasLimited")
            }
        }
    }
}

impl ToBytes for PricingHandling {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;

        match self {
            PricingHandling::Classic => {
                buffer.push(PRICING_HANDLING_CLASSIC_TAG);
            }
            PricingHandling::Fixed => {
                buffer.push(PRICING_HANDLING_FIXED_TAG);
            }
            PricingHandling::GasLimited => {
                buffer.push(PRICING_HANDLING_GASLIMITED_TAG);
            }
        }

        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        PRICING_HANDLING_TAG_LENGTH as usize
    }
}

impl FromBytes for PricingHandling {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            PRICING_HANDLING_CLASSIC_TAG => Ok((PricingHandling::Classic, rem)),
            PRICING_HANDLING_FIXED_TAG => Ok((PricingHandling::Fixed, rem)),
            PRICING_HANDLING_GASLIMITED_TAG => Ok((PricingHandling::GasLimited, rem)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;
    use serde_json::json;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip_for_classic() {
        let handling = PricingHandling::Classic;
        bytesrepr::test_serialization_roundtrip(&handling);
    }

    #[test]
    fn bytesrepr_roundtrip_for_fixed() {
        let handling = PricingHandling::Fixed;
        bytesrepr::test_serialization_roundtrip(&handling);
    }

    #[test]
    fn bytesrepr_roundtrip_for_gas_limited() {
        let handling = PricingHandling::GasLimited;
        bytesrepr::test_serialization_roundtrip(&handling);
    }
}
