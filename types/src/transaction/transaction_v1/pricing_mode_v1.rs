use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::TransactionV1;
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

const GAS_PRICE_MULTIPLIER_TAG: u8 = 0;
const FIXED_TAG: u8 = 1;
const RESERVED_TAG: u8 = 2;

/// The pricing mode of a [`TransactionV1`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Pricing mode of a TransactionV1.")
)]
#[serde(deny_unknown_fields)]
pub enum PricingModeV1 {
    /// Multiplies the gas used by the given amount.
    ///
    /// This is the same behaviour as for the `Deploy::gas_price`.
    GasPriceMultiplier(u64),
    /// First-in-first-out handling of transactions, i.e. pricing mode is irrelevant to ordering.
    Fixed,
    /// The payment for this transaction was previously reserved.
    Reserved,
}

impl PricingModeV1 {
    /// Returns a random `PricingModeV1`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => PricingModeV1::GasPriceMultiplier(rng.gen()),
            1 => PricingModeV1::Fixed,
            2 => PricingModeV1::Reserved,
            _ => unreachable!(),
        }
    }
}

impl Display for PricingModeV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            PricingModeV1::GasPriceMultiplier(multiplier) => {
                write!(formatter, "gas price multiplier {}", multiplier)
            }
            PricingModeV1::Fixed => write!(formatter, "fixed pricing"),
            PricingModeV1::Reserved => write!(formatter, "reserved"),
        }
    }
}

impl ToBytes for PricingModeV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            PricingModeV1::GasPriceMultiplier(multiplier) => {
                GAS_PRICE_MULTIPLIER_TAG.write_bytes(writer)?;
                multiplier.write_bytes(writer)
            }
            PricingModeV1::Fixed => FIXED_TAG.write_bytes(writer),
            PricingModeV1::Reserved => RESERVED_TAG.write_bytes(writer),
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                PricingModeV1::GasPriceMultiplier(multiplier) => multiplier.serialized_length(),
                PricingModeV1::Fixed | PricingModeV1::Reserved => 0,
            }
    }
}

impl FromBytes for PricingModeV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            GAS_PRICE_MULTIPLIER_TAG => {
                let (multiplier, remainder) = u64::from_bytes(remainder)?;
                Ok((PricingModeV1::GasPriceMultiplier(multiplier), remainder))
            }
            FIXED_TAG => Ok((PricingModeV1::Fixed, remainder)),
            RESERVED_TAG => Ok((PricingModeV1::Reserved, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&PricingModeV1::random(rng));
        }
    }
}
