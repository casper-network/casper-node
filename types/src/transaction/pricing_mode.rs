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
use super::Transaction;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest,
};

const CLASSIC_TAG: u8 = 0;
const FIXED_TAG: u8 = 1;
const RESERVED_TAG: u8 = 2;

/// The pricing mode of a [`Transaction`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Pricing mode of a Transaction.")
)]
#[serde(deny_unknown_fields)]
pub enum PricingMode {
    /// The original payment model, where the creator of the transaction
    /// specifies how much they will pay, at what gas price.
    Classic {
        /// User-specified payment amount.
        payment_amount: u64,
        /// User-specified gas_price (minimum 1).
        gas_price: u8,
        /// Standard payment.
        standard_payment: bool,
    },
    /// The cost of the transaction is determined by the cost table, per the
    /// transaction kind.
    Fixed {
        /// User-specified gas_price tolerance (minimum 1).
        /// This is interpreted to mean "do not include this transaction in a block
        /// if the current gas price is greater than this number"
        gas_price_tolerance: u8,
    },
    /// The payment for this transaction was previously reserved, as proven by
    /// the receipt hash (this is for future use, not currently implemented).
    Reserved {
        /// Pre-paid receipt.
        receipt: Digest,
        /// Price paid in the past to reserve space in a future block.
        paid_amount: u64,
        /// The gas price at the time of reservation.
        strike_price: u8,
    },
}

impl PricingMode {
    /// Returns a random `PricingMode.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => PricingMode::Classic {
                payment_amount: rng.gen(),
                gas_price: 1,
                standard_payment: true,
            },
            1 => PricingMode::Fixed {
                gas_price_tolerance: rng.gen(),
            },
            2 => PricingMode::Reserved {
                receipt: rng.gen(),
                paid_amount: rng.gen(),
                strike_price: rng.gen(),
            },
            _ => unreachable!(),
        }
    }
}

impl Display for PricingMode {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            PricingMode::Classic {
                payment_amount,
                gas_price,
                standard_payment,
            } => {
                write!(
                    formatter,
                    "payment amount {}, gas price multiplier {} standard_payment {}",
                    payment_amount, gas_price, standard_payment
                )
            }
            PricingMode::Reserved {
                receipt,
                paid_amount,
                strike_price,
            } => write!(
                formatter,
                "reserved: {} paid_amount: {} strike_price: {}",
                receipt, paid_amount, strike_price
            ),
            PricingMode::Fixed {
                gas_price_tolerance,
            } => write!(formatter, "fixed pricing {}", gas_price_tolerance),
        }
    }
}

impl ToBytes for PricingMode {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            PricingMode::Classic {
                payment_amount,
                gas_price,
                standard_payment,
            } => {
                CLASSIC_TAG.write_bytes(writer)?;
                payment_amount.write_bytes(writer)?;
                gas_price.write_bytes(writer)?;
                standard_payment.write_bytes(writer)
            }
            PricingMode::Reserved {
                receipt,
                paid_amount,
                strike_price,
            } => {
                RESERVED_TAG.write_bytes(writer)?;
                receipt.write_bytes(writer)?;
                paid_amount.write_bytes(writer)?;
                strike_price.write_bytes(writer)
            }
            PricingMode::Fixed {
                gas_price_tolerance,
            } => {
                FIXED_TAG.write_bytes(writer)?;
                gas_price_tolerance.write_bytes(writer)
            }
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
                PricingMode::Classic {
                    payment_amount,
                    gas_price,
                    standard_payment,
                } => {
                    payment_amount.serialized_length()
                        + gas_price.serialized_length()
                        + standard_payment.serialized_length()
                }
                PricingMode::Reserved {
                    receipt,
                    paid_amount,
                    strike_price,
                } => {
                    receipt.serialized_length()
                        + paid_amount.serialized_length()
                        + strike_price.serialized_length()
                }
                PricingMode::Fixed {
                    gas_price_tolerance,
                } => gas_price_tolerance.serialized_length(),
            }
    }
}

impl FromBytes for PricingMode {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;

        match tag {
            CLASSIC_TAG => {
                let (payment_amount, remainder) = u64::from_bytes(remainder)?;
                let (gas_price, remainder) = u8::from_bytes(remainder)?;
                let (standard_payment, remainder) = bool::from_bytes(remainder)?;
                Ok((
                    PricingMode::Classic {
                        payment_amount,
                        gas_price,
                        standard_payment,
                    },
                    remainder,
                ))
            }
            FIXED_TAG => {
                let (gas_price_tolerance, remainder) = u8::from_bytes(remainder)?;
                Ok((
                    PricingMode::Fixed {
                        gas_price_tolerance,
                    },
                    remainder,
                ))
            }
            RESERVED_TAG => {
                let (receipt, remainder) = Digest::from_bytes(remainder)?;
                let (paid_amount, remainder) = u64::from_bytes(remainder)?;
                let (strike_price, remainder) = u8::from_bytes(remainder)?;
                Ok((
                    PricingMode::Reserved {
                        receipt,
                        paid_amount,
                        strike_price,
                    },
                    remainder,
                ))
            }
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
            bytesrepr::test_serialization_roundtrip(&PricingMode::random(rng));
        }
    }
}
