use datasize::DataSize;
use num::CheckedMul;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use super::gas::Gas;

pub const DEFAULT_GAS_PER_BYTE_COST: u32 = 625_000;

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct StorageCosts {
    /// Gas charged per byte stored in the global state.
    gas_per_byte: u32,
}

impl StorageCosts {
    pub const fn new(gas_per_byte: u32) -> Self {
        Self { gas_per_byte }
    }

    pub fn gas_per_byte(&self) -> u32 {
        self.gas_per_byte
    }

    /// Calculates gas cost for storing `bytes`.
    ///
    /// Returns wrapped [`Gas`] if calculation did not overflow. This method will return [`None`] if
    /// the result would overflow [`Gas`] range - in this case the contract execution logic should
    /// exit early with a gas limit error.
    pub fn calculate_gas_cost(&self, bytes: usize) -> Option<Gas> {
        let lhs = Gas::from(self.gas_per_byte);
        let rhs = Gas::from(bytes);
        let gas_cost = lhs.checked_mul(&rhs)?;
        Some(gas_cost)
    }
}

impl Default for StorageCosts {
    fn default() -> Self {
        Self {
            gas_per_byte: DEFAULT_GAS_PER_BYTE_COST,
        }
    }
}

impl Distribution<StorageCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> StorageCosts {
        StorageCosts {
            gas_per_byte: rng.gen(),
        }
    }
}

impl ToBytes for StorageCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.gas_per_byte.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.gas_per_byte.serialized_length()
    }
}

impl FromBytes for StorageCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (gas_per_byte, rem) = FromBytes::from_bytes(bytes)?;

        Ok((StorageCosts { gas_per_byte }, rem))
    }
}

#[cfg(test)]
pub mod tests {
    use casper_types::U512;

    use super::*;

    const SMALL_WEIGHT: usize = 123456789;
    const LARGE_WEIGHT: usize = u64::max_value() as usize;

    #[test]
    fn should_calculate_gas_cost() {
        let storage_costs = StorageCosts::default();

        let cost = storage_costs.calculate_gas_cost(SMALL_WEIGHT);

        let expected_cost = (DEFAULT_GAS_PER_BYTE_COST as u64) * (SMALL_WEIGHT as u64);
        assert_eq!(cost, Some(Gas::new(expected_cost)));
    }

    #[test]
    fn should_calculate_big_gas_cost() {
        let storage_costs = StorageCosts::default();

        let cost = storage_costs.calculate_gas_cost(LARGE_WEIGHT);

        let expected_cost = U512::from(DEFAULT_GAS_PER_BYTE_COST) * U512::from(LARGE_WEIGHT);

        assert!(expected_cost > U512::from(u64::MAX));

        assert_eq!(cost, None);
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use super::StorageCosts;

    prop_compose! {
        pub fn storage_costs_arb()(
            gas_per_byte in num::u32::ANY,
        ) -> StorageCosts {
            StorageCosts {
                gas_per_byte,
            }
        }
    }
}
