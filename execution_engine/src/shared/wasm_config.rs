//! Configuration of the Wasm execution engine.
use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use super::{
    host_function_costs::HostFunctionCosts, opcode_costs::OpcodeCosts, storage_costs::StorageCosts,
};

/// Default maximum number of pages of the Wasm memory.
pub const DEFAULT_WASM_MAX_MEMORY: u32 = 64;
/// Default maximum stack height.
pub const DEFAULT_MAX_STACK_HEIGHT: u32 = 188;

/// Configuration of the Wasm execution environment.
///
/// This structure contains various Wasm execution configuration options, such as memory limits,
/// stack limits and costs.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct WasmConfig {
    /// Maximum amount of heap memory (represented in 64kB pages) each contract can use.
    pub max_memory: u32,
    /// Max stack height (native WebAssembly stack limiter).
    pub max_stack_height: u32,
    /// Wasm opcode costs table.
    opcode_costs: OpcodeCosts,
    /// Storage costs.
    storage_costs: StorageCosts,
    /// Host function costs table.
    host_function_costs: HostFunctionCosts,
}

impl WasmConfig {
    /// Creates new Wasm config.
    pub const fn new(
        max_memory: u32,
        max_stack_height: u32,
        opcode_costs: OpcodeCosts,
        storage_costs: StorageCosts,
        host_function_costs: HostFunctionCosts,
    ) -> Self {
        Self {
            max_memory,
            max_stack_height,
            opcode_costs,
            storage_costs,
            host_function_costs,
        }
    }

    /// Returns opcode costs.
    pub fn opcode_costs(&self) -> OpcodeCosts {
        self.opcode_costs
    }

    /// Returns storage costs.
    pub fn storage_costs(&self) -> StorageCosts {
        self.storage_costs
    }

    /// Returns host function costs and consumes this object.
    pub fn take_host_function_costs(self) -> HostFunctionCosts {
        self.host_function_costs
    }
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            max_memory: DEFAULT_WASM_MAX_MEMORY,
            max_stack_height: DEFAULT_MAX_STACK_HEIGHT,
            opcode_costs: OpcodeCosts::default(),
            storage_costs: StorageCosts::default(),
            host_function_costs: HostFunctionCosts::default(),
        }
    }
}

impl ToBytes for WasmConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.max_memory.to_bytes()?);
        ret.append(&mut self.max_stack_height.to_bytes()?);
        ret.append(&mut self.opcode_costs.to_bytes()?);
        ret.append(&mut self.storage_costs.to_bytes()?);
        ret.append(&mut self.host_function_costs.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.max_memory.serialized_length()
            + self.max_stack_height.serialized_length()
            + self.opcode_costs.serialized_length()
            + self.storage_costs.serialized_length()
            + self.host_function_costs.serialized_length()
    }
}

impl FromBytes for WasmConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_memory, rem) = FromBytes::from_bytes(bytes)?;
        let (max_stack_height, rem) = FromBytes::from_bytes(rem)?;
        let (opcode_costs, rem) = FromBytes::from_bytes(rem)?;
        let (storage_costs, rem) = FromBytes::from_bytes(rem)?;
        let (host_function_costs, rem) = FromBytes::from_bytes(rem)?;

        Ok((
            WasmConfig {
                max_memory,
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
            max_memory: rng.gen(),
            max_stack_height: rng.gen(),
            opcode_costs: rng.gen(),
            storage_costs: rng.gen(),
            host_function_costs: rng.gen(),
        }
    }
}

#[doc(hidden)]
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
            max_memory in num::u32::ANY,
            max_stack_height in num::u32::ANY,
            opcode_costs in opcode_costs_arb(),
            storage_costs in storage_costs_arb(),
            host_function_costs in host_function_costs_arb(),
        ) -> WasmConfig {
            WasmConfig {
                max_memory,
                max_stack_height,
                opcode_costs,
                storage_costs,
                host_function_costs,
            }
        }
    }
}
