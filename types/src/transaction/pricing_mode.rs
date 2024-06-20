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
const GAS_LIMITED_TAG: u8 = 3;

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
        /// User-specified gas_price tolerance (minimum 1).
        /// This is interpreted to mean "do not include this transaction in a block
        /// if the current gas price is greater than this number"
        gas_price_tolerance: u8,
        /// Standard payment.
        standard_payment: bool,
    },
    /// The cost of the transaction is determined by the cost table, per the
    /// transaction category.
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
    },
    /// Gas limited transaction.
    GasLimited {
        /// User-specified gas limit.
        gas_limit: u64,
        /// User-specified gas_price tolerance (minimum 1).
        /// This is interpreted to mean "do not include this transaction in a block
        /// if the current gas price is greater than this number"
        gas_price_tolerance: u8,
    },
}

impl PricingMode {
    /// Returns a random `PricingMode.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..=3) {
            0 => PricingMode::Classic {
                payment_amount: rng.gen(),
                gas_price_tolerance: 1,
                standard_payment: true,
            },
            1 => PricingMode::Fixed {
                gas_price_tolerance: rng.gen(),
            },
            2 => PricingMode::Reserved { receipt: rng.gen() },
            3 => PricingMode::GasLimited {
                gas_limit: rng.gen(),
                gas_price_tolerance: rng.gen(),
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
                gas_price_tolerance: gas_price,
                standard_payment,
            } => {
                write!(
                    formatter,
                    "payment amount {}, gas price multiplier {} standard_payment {}",
                    payment_amount, gas_price, standard_payment
                )
            }
            PricingMode::Reserved { receipt } => write!(formatter, "reserved: {}", receipt),
            PricingMode::Fixed {
                gas_price_tolerance,
            } => write!(formatter, "fixed pricing {}", gas_price_tolerance),
            PricingMode::GasLimited {
                gas_limit,
                gas_price_tolerance,
            } => {
                write!(
                    formatter,
                    "gas limit {}, gas price multiplier {}",
                    gas_limit, gas_price_tolerance
                )
            }
        }
    }
}

impl ToBytes for PricingMode {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            PricingMode::Classic {
                payment_amount,
                gas_price_tolerance: gas_price,
                standard_payment,
            } => {
                CLASSIC_TAG.write_bytes(writer)?;
                payment_amount.write_bytes(writer)?;
                gas_price.write_bytes(writer)?;
                standard_payment.write_bytes(writer)
            }
            PricingMode::Reserved { receipt } => {
                RESERVED_TAG.write_bytes(writer)?;
                receipt.write_bytes(writer)
            }
            PricingMode::Fixed {
                gas_price_tolerance,
            } => {
                FIXED_TAG.write_bytes(writer)?;
                gas_price_tolerance.write_bytes(writer)
            }
            PricingMode::GasLimited {
                gas_limit,
                gas_price_tolerance,
            } => {
                GAS_LIMITED_TAG.write_bytes(writer)?;
                gas_limit.write_bytes(writer)?;
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
                    gas_price_tolerance: gas_price,
                    standard_payment,
                } => {
                    payment_amount.serialized_length()
                        + gas_price.serialized_length()
                        + standard_payment.serialized_length()
                }
                PricingMode::Reserved { receipt } => receipt.serialized_length(),
                PricingMode::Fixed {
                    gas_price_tolerance,
                } => gas_price_tolerance.serialized_length(),
                PricingMode::GasLimited {
                    gas_limit,
                    gas_price_tolerance,
                } => gas_limit.serialized_length() + gas_price_tolerance.serialized_length(),
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
                        gas_price_tolerance: gas_price,
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
                Ok((PricingMode::Reserved { receipt }, remainder))
            }
            GAS_LIMITED_TAG => {
                let (gas_limit, remainder) = u64::from_bytes(remainder)?;
                let (gas_price_tolerance, remainder) = u8::from_bytes(remainder)?;
                Ok((
                    PricingMode::GasLimited {
                        gas_limit,
                        gas_price_tolerance,
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
    #[test]
    fn test_to_bytes_and_from_bytes() {
        let classic = PricingMode::Classic {
            payment_amount: 100,
            gas_price_tolerance: 1,
            standard_payment: true,
        };
        match classic {
            PricingMode::Classic { .. } => {}
            PricingMode::Fixed { .. } => {}
            PricingMode::Reserved { .. } => {}
            PricingMode::GasLimited { .. } => {}
        }
        bytesrepr::test_serialization_roundtrip(&classic);
        bytesrepr::test_serialization_roundtrip(&PricingMode::Fixed {
            gas_price_tolerance: 2,
        });
        bytesrepr::test_serialization_roundtrip(&PricingMode::Reserved {
            receipt: Digest::default(),
        });
        bytesrepr::test_serialization_roundtrip(&PricingMode::GasLimited {
            gas_limit: 200,
            gas_price_tolerance: 3,
        });
    }
}
