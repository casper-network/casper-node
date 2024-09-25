use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::serialization::CalltableSerializationEnvelope;
#[cfg(doc)]
use super::Transaction;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    transaction::serialization::CalltableSerializationEnvelopeBuilder,
    Digest,
};

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
    PaymentLimited {
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
}

impl PricingMode {
    /// Returns a random `PricingMode.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..=2) {
            0 => PricingMode::PaymentLimited {
                payment_amount: rng.gen(),
                gas_price_tolerance: 1,
                standard_payment: true,
            },
            1 => PricingMode::Fixed {
                gas_price_tolerance: rng.gen(),
            },
            2 => PricingMode::Reserved { receipt: rng.gen() },
            _ => unreachable!(),
        }
    }

    /// Returns standard payment flag, if it is a `PaymentLimited` variant.
    pub fn is_standard_payment(&self) -> bool {
        match self {
            PricingMode::PaymentLimited {
                standard_payment, ..
            } => *standard_payment,
            PricingMode::Fixed { .. } => true,
            PricingMode::Reserved { .. } => true,
        }
    }

    fn serialized_field_lengths(&self) -> Vec<usize> {
        match self {
            PricingMode::PaymentLimited {
                payment_amount,
                gas_price_tolerance,
                standard_payment,
            } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    payment_amount.serialized_length(),
                    gas_price_tolerance.serialized_length(),
                    standard_payment.serialized_length(),
                ]
            }
            PricingMode::Fixed {
                gas_price_tolerance,
            } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    gas_price_tolerance.serialized_length(),
                ]
            }
            PricingMode::Reserved { receipt } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    receipt.serialized_length(),
                ]
            }
        }
    }
}
const TAG_FIELD_INDEX: u16 = 0;

const PAYMENT_LIMITED_VARIANT_TAG: u8 = 0;
const PAYMENT_LIMITED_PAYMENT_AMOUNT_INDEX: u16 = 1;
const PAYMENT_LIMITED_GAS_PRICE_TOLERANCE_INDEX: u16 = 2;
const PAYMENT_LIMITED_STANDARD_PAYMENT_INDEX: u16 = 3;

const FIXED_VARIANT_TAG: u8 = 1;
const FIXED_GAS_PRICE_TOLERANCE_INDEX: u16 = 1;

const RESERVED_VARIANT_TAG: u8 = 2;
const RESERVED_RECEIPT_INDEX: u16 = 1;

impl ToBytes for PricingMode {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            PricingMode::PaymentLimited {
                payment_amount,
                gas_price_tolerance,
                standard_payment,
            } => CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                .add_field(TAG_FIELD_INDEX, &PAYMENT_LIMITED_VARIANT_TAG)?
                .add_field(PAYMENT_LIMITED_PAYMENT_AMOUNT_INDEX, &payment_amount)?
                .add_field(
                    PAYMENT_LIMITED_GAS_PRICE_TOLERANCE_INDEX,
                    &gas_price_tolerance,
                )?
                .add_field(PAYMENT_LIMITED_STANDARD_PAYMENT_INDEX, &standard_payment)?
                .binary_payload_bytes(),
            PricingMode::Fixed {
                gas_price_tolerance,
            } => CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                .add_field(TAG_FIELD_INDEX, &FIXED_VARIANT_TAG)?
                .add_field(FIXED_GAS_PRICE_TOLERANCE_INDEX, &gas_price_tolerance)?
                .binary_payload_bytes(),
            PricingMode::Reserved { receipt } => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &RESERVED_VARIANT_TAG)?
                    .add_field(RESERVED_RECEIPT_INDEX, &receipt)?
                    .binary_payload_bytes()
            }
        }
    }
    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for PricingMode {
    fn from_bytes(bytes: &[u8]) -> Result<(PricingMode, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(4, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Formatting)?;
        window.verify_index(TAG_FIELD_INDEX)?;
        let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
        let to_ret = match tag {
            PAYMENT_LIMITED_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(PAYMENT_LIMITED_PAYMENT_AMOUNT_INDEX)?;
                let (payment_amount, window) = window.deserialize_and_maybe_next::<u64>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(PAYMENT_LIMITED_GAS_PRICE_TOLERANCE_INDEX)?;
                let (gas_price_tolerance, window) = window.deserialize_and_maybe_next::<u8>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(PAYMENT_LIMITED_STANDARD_PAYMENT_INDEX)?;
                let (standard_payment, window) = window.deserialize_and_maybe_next::<bool>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(PricingMode::PaymentLimited {
                    payment_amount,
                    gas_price_tolerance,
                    standard_payment,
                })
            }
            FIXED_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(FIXED_GAS_PRICE_TOLERANCE_INDEX)?;
                let (gas_price_tolerance, window) = window.deserialize_and_maybe_next::<u8>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(PricingMode::Fixed {
                    gas_price_tolerance,
                })
            }
            RESERVED_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(RESERVED_RECEIPT_INDEX)?;
                let (receipt, window) = window.deserialize_and_maybe_next::<Digest>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(PricingMode::Reserved { receipt })
            }
            _ => Err(Formatting),
        };
        to_ret.map(|endpoint| (endpoint, remainder))
    }
}

impl Display for PricingMode {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            PricingMode::PaymentLimited {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr, testing::TestRng};

    #[test]
    fn test_to_bytes_and_from_bytes() {
        let classic = PricingMode::PaymentLimited {
            payment_amount: 100,
            gas_price_tolerance: 1,
            standard_payment: true,
        };
        match classic {
            PricingMode::PaymentLimited { .. } => {}
            PricingMode::Fixed { .. } => {}
            PricingMode::Reserved { .. } => {}
        }
        bytesrepr::test_serialization_roundtrip(&classic);
        bytesrepr::test_serialization_roundtrip(&PricingMode::Fixed {
            gas_price_tolerance: 2,
        });
        bytesrepr::test_serialization_roundtrip(&PricingMode::Reserved {
            receipt: Digest::default(),
        });
    }

    use crate::gens::pricing_mode_arb;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in pricing_mode_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
