//! Costs of the standard payment system contract.
#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// Default cost of the `pay` standard payment entry point.
const DEFAULT_PAY_COST: u32 = 10_000;

/// Description of the costs of calling standard payment entry points.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct StandardPaymentCosts {
    /// Cost of calling the `pay` entry point.
    pub pay: u32,
}

impl Default for StandardPaymentCosts {
    fn default() -> Self {
        Self {
            pay: DEFAULT_PAY_COST,
        }
    }
}

impl ToBytes for StandardPaymentCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.pay.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.pay.serialized_length()
    }
}

impl FromBytes for StandardPaymentCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (pay, rem) = FromBytes::from_bytes(bytes)?;
        Ok((Self { pay }, rem))
    }
}

impl Distribution<StandardPaymentCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> StandardPaymentCosts {
        StandardPaymentCosts { pay: rng.gen() }
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use super::StandardPaymentCosts;

    prop_compose! {
        pub fn standard_payment_costs_arb()(
            pay in num::u32::ANY,
        ) -> StandardPaymentCosts {
            StandardPaymentCosts {
                pay,
            }
        }
    }
}
