use crate::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
};
use core::fmt::{Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};

const HOLD_BALANCE_ACCRUED_TAG: u8 = 0;
const HOLD_BALANCE_AMORTIZED_TAG: u8 = 1;
const HOLD_BALANCE_HANDLING_TAG_LENGTH: u8 = 1;

/// Defines how a given network handles holds when calculating available balances. There may be
/// multiple types of holds (such as Processing and Gas currently, and potentially other kinds in
/// the future), and each type of hold can differ on how it applies to available
/// balance calculation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum HoldBalanceHandling {
    /// The sum of full value of all non-expired holds is used.
    // in 2.0 the default hold balance handling is Accrued,
    // which means a non-expired hold is applied in full to
    // available balance calculations
    #[default]
    Accrued,
    /// The sum of each hold is amortized over the time remaining until expiry.
    /// For instance, if 12 hours remain on a 24 hour hold, half the hold amount is applied.
    Amortized,
}

impl HoldBalanceHandling {
    /// Returns variant for tag, if able.
    #[allow(clippy::result_unit_err)]
    pub fn from_tag(tag: u8) -> Result<HoldBalanceHandling, ()> {
        if tag == HOLD_BALANCE_ACCRUED_TAG {
            Ok(HoldBalanceHandling::Accrued)
        } else if tag == HOLD_BALANCE_AMORTIZED_TAG {
            Ok(HoldBalanceHandling::Amortized)
        } else {
            Err(())
        }
    }

    /// Returns the tag for the variant.
    pub fn tag(&self) -> u8 {
        match self {
            HoldBalanceHandling::Accrued => HOLD_BALANCE_ACCRUED_TAG,
            HoldBalanceHandling::Amortized => HOLD_BALANCE_AMORTIZED_TAG,
        }
    }
}

impl Display for HoldBalanceHandling {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            HoldBalanceHandling::Accrued => {
                write!(f, "HoldBalanceHandling::Accrued")
            }
            HoldBalanceHandling::Amortized => {
                write!(f, "HoldBalanceHandling::Amortized")
            }
        }
    }
}

impl ToBytes for HoldBalanceHandling {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;

        match self {
            HoldBalanceHandling::Accrued => {
                buffer.push(HOLD_BALANCE_ACCRUED_TAG);
            }
            HoldBalanceHandling::Amortized => {
                buffer.push(HOLD_BALANCE_AMORTIZED_TAG);
            }
        }

        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        HOLD_BALANCE_HANDLING_TAG_LENGTH as usize
    }
}

impl FromBytes for HoldBalanceHandling {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            HOLD_BALANCE_ACCRUED_TAG => Ok((HoldBalanceHandling::Accrued, rem)),
            HOLD_BALANCE_AMORTIZED_TAG => Ok((HoldBalanceHandling::Amortized, rem)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<HoldBalanceHandling> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> HoldBalanceHandling {
        match rng.gen_range(HOLD_BALANCE_ACCRUED_TAG..=HOLD_BALANCE_AMORTIZED_TAG) {
            HOLD_BALANCE_ACCRUED_TAG => HoldBalanceHandling::Accrued,
            HOLD_BALANCE_AMORTIZED_TAG => HoldBalanceHandling::Amortized,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip_for_accrued() {
        let handling = HoldBalanceHandling::Accrued;
        bytesrepr::test_serialization_roundtrip(&handling);
    }

    #[test]
    fn bytesrepr_roundtrip_for_amortized() {
        let handling = HoldBalanceHandling::Amortized;
        bytesrepr::test_serialization_roundtrip(&handling);
    }
}
