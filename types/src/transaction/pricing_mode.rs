use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::serialization::pricing_mode::{
    classic_serialized_length, deserialize_pricing_mode, fixed_serialized_length,
    reserved_serialized_length, serialize_classic_pricing_mode, serialize_fixed_pricing_mode,
    serialize_reserved_pricing_mode,
};
#[cfg(doc)]
use super::Transaction;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
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
}

impl PricingMode {
    /// Returns a random `PricingMode.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => PricingMode::Classic {
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
        }
    }
}

impl ToBytes for PricingMode {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            PricingMode::Classic {
                payment_amount,
                gas_price_tolerance: gas_price,
                standard_payment,
            } => serialize_classic_pricing_mode(payment_amount, gas_price, standard_payment),
            PricingMode::Reserved { receipt } => serialize_reserved_pricing_mode(receipt),
            PricingMode::Fixed {
                gas_price_tolerance,
            } => serialize_fixed_pricing_mode(gas_price_tolerance),
        }
    }

    fn serialized_length(&self) -> usize {
        match self {
            PricingMode::Classic {
                payment_amount,
                gas_price_tolerance,
                standard_payment,
            } => classic_serialized_length(payment_amount, gas_price_tolerance, standard_payment),
            PricingMode::Reserved { receipt } => reserved_serialized_length(receipt),
            PricingMode::Fixed {
                gas_price_tolerance,
            } => fixed_serialized_length(gas_price_tolerance),
        }
    }
}

impl FromBytes for PricingMode {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        deserialize_pricing_mode(bytes)
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

    use crate::gens::pricing_mode_arb;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in pricing_mode_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
