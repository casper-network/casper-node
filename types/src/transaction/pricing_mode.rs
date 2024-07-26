use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::serialization::{BinaryPayload, CalltableFromBytes, CalltableToBytes};
#[cfg(doc)]
use super::Transaction;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, ToBytes},
    Digest,
};
use macros::{CalltableFromBytes, CalltableToBytes};

/// The pricing mode of a [`Transaction`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Pricing mode of a Transaction.")
)]
#[serde(deny_unknown_fields)]
#[derive(CalltableToBytes, CalltableFromBytes)]
pub enum PricingMode {
    /// The original payment model, where the creator of the transaction
    /// specifies how much they will pay, at what gas price.
    #[calltable(variant_index = 0)]
    Classic {
        /// User-specified payment amount.
        #[calltable(field_index = 1)]
        payment_amount: u64,
        /// User-specified gas_price tolerance (minimum 1).
        /// This is interpreted to mean "do not include this transaction in a block
        /// if the current gas price is greater than this number"
        #[calltable(field_index = 2)]
        gas_price_tolerance: u8,
        /// Standard payment.
        #[calltable(field_index = 3)]
        standard_payment: bool,
    },
    /// The cost of the transaction is determined by the cost table, per the
    /// transaction category.
    #[calltable(variant_index = 1)]
    Fixed {
        /// User-specified gas_price tolerance (minimum 1).
        /// This is interpreted to mean "do not include this transaction in a block
        /// if the current gas price is greater than this number"
        #[calltable(field_index = 1)]
        gas_price_tolerance: u8,
    },
    /// The payment for this transaction was previously reserved, as proven by
    /// the receipt hash (this is for future use, not currently implemented).
    #[calltable(variant_index = 2)]
    Reserved {
        /// Pre-paid receipt.
        #[calltable(field_index = 1)]
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
