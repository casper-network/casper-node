use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

pub const DEFAULT_GAS_PER_BYTE_COST: u32 = 625_000;

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct StorageCosts {
    /// Gas charged per byte stored in the global state.
    pub gas_per_byte: u32,
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
