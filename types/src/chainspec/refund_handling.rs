/// Configuration options of refund handling that are executed as part of handle payment
/// finalization.
use num_rational::Ratio;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

const REFUND_HANDLING_REFUND_TAG: u8 = 0;
const REFUND_HANDLING_BURN_TAG: u8 = 1;
const REFUND_HANDLING_NONE_TAG: u8 = 2;

/// Defines how refunds are calculated.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RefundHandling {
    /// Refund of excess payment amount goes to either a pre-defined purse, or back to the sender
    /// and the rest of the payment amount goes to the block proposer.
    Refund {
        /// Computes how much refund goes back to the user after deducting gas spent from the paid
        /// amount.
        ///
        /// user_part = (payment_amount - gas_spent_amount) * refund_ratio
        /// validator_part = payment_amount - user_part
        ///
        /// Any dust amount that was a result of multiplying by refund_ratio goes back to user.
        refund_ratio: Ratio<u64>,
    },
    /// Burns the refund amount.
    Burn {
        /// Computes how much of the refund amount is burned after deducting gas spent from the
        /// paid amount.
        refund_ratio: Ratio<u64>,
    },
    /// No refunds.
    None,
}

impl ToBytes for RefundHandling {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;

        match self {
            RefundHandling::Refund { refund_ratio } => {
                buffer.push(REFUND_HANDLING_REFUND_TAG);
                buffer.extend(refund_ratio.to_bytes()?);
            }
            RefundHandling::Burn { refund_ratio } => {
                buffer.push(REFUND_HANDLING_BURN_TAG);
                buffer.extend(refund_ratio.to_bytes()?);
            }
            RefundHandling::None => {
                buffer.push(REFUND_HANDLING_NONE_TAG);
            }
        }

        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        1 + match self {
            RefundHandling::Refund { refund_ratio } => refund_ratio.serialized_length(),
            RefundHandling::Burn { refund_ratio } => refund_ratio.serialized_length(),
            RefundHandling::None => 0,
        }
    }
}

impl FromBytes for RefundHandling {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            REFUND_HANDLING_REFUND_TAG => {
                let (refund_ratio, rem) = FromBytes::from_bytes(rem)?;
                Ok((RefundHandling::Refund { refund_ratio }, rem))
            }
            REFUND_HANDLING_BURN_TAG => {
                let (refund_ratio, rem) = FromBytes::from_bytes(rem)?;
                Ok((RefundHandling::Burn { refund_ratio }, rem))
            }
            REFUND_HANDLING_NONE_TAG => Ok((RefundHandling::None, rem)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Default for RefundHandling {
    fn default() -> Self {
        // in 2.0 the default payment mode is Fixed with Fee Elimination on,
        // thus there is nothing to refund.
        // RefundHandling::None
        // in 1.x the default was Refund
        RefundHandling::Refund {
            refund_ratio: Ratio::new(99, 100),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip_for_refund() {
        let refund_config = RefundHandling::Refund {
            refund_ratio: Ratio::new(49, 313),
        };
        bytesrepr::test_serialization_roundtrip(&refund_config);
    }

    #[test]
    fn bytesrepr_roundtrip_for_burn() {
        let refund_config = RefundHandling::Burn {
            refund_ratio: Ratio::new(49, 313),
        };
        bytesrepr::test_serialization_roundtrip(&refund_config);
    }
}
