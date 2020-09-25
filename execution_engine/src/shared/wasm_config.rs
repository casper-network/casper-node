use bytesrepr::{FromBytes, ToBytes};
use casper_types::bytesrepr;
use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use super::{
    host_function_costs::HostFunctionCosts, opcode_costs::OpCodeCosts, storage_costs::StorageCosts,
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct WasmConfig {
    /// Memory stipend. Amount of free memory (in 64kb pages) each contract can
    /// use for stack.
    pub initial_mem: u32,
    /// Max stack height (native WebAssembly stack limiter)
    pub max_stack_height: u32,
    /// Wasm opcode costs table
    pub opcode_costs: OpCodeCosts,
    /// Storage costs
    pub storage_costs: StorageCosts,
    /// Host function costs table
    pub host_function_costs: HostFunctionCosts,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            initial_mem: 64,
            max_stack_height: 64 * 1024,
            opcode_costs: OpCodeCosts::default(),
            storage_costs: StorageCosts::default(),
            host_function_costs: HostFunctionCosts::default(),
        }
    }
}

impl ToBytes for WasmConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.initial_mem.to_bytes()?);
        ret.append(&mut self.max_stack_height.to_bytes()?);
        ret.append(&mut self.opcode_costs.to_bytes()?);
        ret.append(&mut self.storage_costs.to_bytes()?);
        ret.append(&mut self.host_function_costs.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.initial_mem.serialized_length()
            + self.max_stack_height.serialized_length()
            + self.opcode_costs.serialized_length()
            + self.storage_costs.serialized_length()
            + self.host_function_costs.serialized_length()
    }
}

impl FromBytes for WasmConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (initial_mem, rem) = FromBytes::from_bytes(bytes)?;
        let (max_stack_height, rem) = FromBytes::from_bytes(rem)?;
        let (opcode_costs, rem) = FromBytes::from_bytes(rem)?;
        let (storage_costs, rem) = FromBytes::from_bytes(rem)?;
        let (host_function_costs, rem) = FromBytes::from_bytes(rem)?;

        Ok((
            WasmConfig {
                initial_mem,
                max_stack_height,
                opcode_costs,
                storage_costs,
                host_function_costs,
            },
            rem,
        ))
    }
}

impl Distribution<WasmConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> WasmConfig {
        WasmConfig {
            initial_mem: rng.gen(),
            max_stack_height: rng.gen(),
            opcode_costs: rng.gen(),
            storage_costs: rng.gen(),
            host_function_costs: rng.gen(),
        }
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use super::WasmConfig;
    use crate::shared::{
        host_function_costs::gens::host_function_costs_arb, opcode_costs::gens::opcode_costs_arb,
        storage_costs::gens::storage_costs_arb,
    };

    prop_compose! {
        pub fn wasm_config_arb() (
            initial_mem in num::u32::ANY,
            max_stack_height in num::u32::ANY,
            opcode_costs in opcode_costs_arb(),
            storage_costs in storage_costs_arb(),
            host_function_costs in host_function_costs_arb(),
        ) -> WasmConfig {
            WasmConfig {
                initial_mem,
                max_stack_height,
                opcode_costs,
                storage_costs,
                host_function_costs,
            }
        }
    }
}
