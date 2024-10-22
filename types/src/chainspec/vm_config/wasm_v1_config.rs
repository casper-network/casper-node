use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    chainspec::vm_config::{HostFunctionCosts, OpcodeCosts},
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

/// Default maximum number of pages of the Wasm memory.
pub const DEFAULT_V1_WASM_MAX_MEMORY: u32 = 64;
/// Default maximum stack height.
pub const DEFAULT_V1_MAX_STACK_HEIGHT: u32 = 500;

/// Configuration of the Wasm execution environment for V1 execution machine.
///
/// This structure contains various Wasm execution configuration options, such as memory limits,
/// stack limits and costs.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct WasmV1Config {
    /// Maximum amount of heap memory (represented in 64kB pages) each contract can use.
    max_memory: u32,
    /// Max stack height (native WebAssembly stack limiter).
    max_stack_height: u32,
    /// Wasm opcode costs table.
    opcode_costs: OpcodeCosts,
    /// Host function costs table.
    host_function_costs: HostFunctionCosts,
}

impl WasmV1Config {
    /// ctor
    pub fn new(
        max_memory: u32,
        max_stack_height: u32,
        opcode_costs: OpcodeCosts,
        host_function_costs: HostFunctionCosts,
    ) -> Self {
        WasmV1Config {
            max_memory,
            max_stack_height,
            opcode_costs,
            host_function_costs,
        }
    }

    /// Returns opcode costs.
    pub fn opcode_costs(&self) -> OpcodeCosts {
        self.opcode_costs
    }

    /// Returns host function costs and consumes this object.
    pub fn take_host_function_costs(self) -> HostFunctionCosts {
        self.host_function_costs
    }

    /// Returns max_memory.
    pub fn max_memory(&self) -> u32 {
        self.max_memory
    }

    /// Returns mutable max_memory reference
    #[cfg(any(feature = "testing", test))]
    pub fn max_memory_mut(&mut self) -> &mut u32 {
        &mut self.max_memory
    }

    /// Returns mutable max_stack_height reference
    #[cfg(any(feature = "testing", test))]
    pub fn max_stack_height_mut(&mut self) -> &mut u32 {
        &mut self.max_stack_height
    }

    /// Returns max_stack_height.
    pub fn max_stack_height(&self) -> u32 {
        self.max_stack_height
    }
}

impl Default for WasmV1Config {
    fn default() -> Self {
        Self {
            max_memory: DEFAULT_V1_WASM_MAX_MEMORY,
            max_stack_height: DEFAULT_V1_MAX_STACK_HEIGHT,
            opcode_costs: OpcodeCosts::default(),
            host_function_costs: HostFunctionCosts::default(),
        }
    }
}

impl ToBytes for WasmV1Config {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.max_memory.to_bytes()?);
        ret.append(&mut self.max_stack_height.to_bytes()?);
        ret.append(&mut self.opcode_costs.to_bytes()?);
        ret.append(&mut self.host_function_costs.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.max_memory.serialized_length()
            + self.max_stack_height.serialized_length()
            + self.opcode_costs.serialized_length()
            + self.host_function_costs.serialized_length()
    }
}

impl FromBytes for WasmV1Config {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_memory, rem) = FromBytes::from_bytes(bytes)?;
        let (max_stack_height, rem) = FromBytes::from_bytes(rem)?;
        let (opcode_costs, rem) = FromBytes::from_bytes(rem)?;
        let (host_function_costs, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            WasmV1Config {
                max_memory,
                max_stack_height,
                opcode_costs,
                host_function_costs,
            },
            rem,
        ))
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<WasmV1Config> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> WasmV1Config {
        WasmV1Config {
            max_memory: rng.gen(),
            max_stack_height: rng.gen(),
            opcode_costs: rng.gen(),
            host_function_costs: rng.gen(),
        }
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use crate::{
        chainspec::vm_config::{
            host_function_costs::gens::host_function_costs_arb,
            opcode_costs::gens::opcode_costs_arb,
        },
        gens::example_u32_arb,
    };
    use proptest::prop_compose;

    use super::WasmV1Config;

    prop_compose! {
        pub fn wasm_v1_config_arb() (
            max_memory in example_u32_arb(),
            max_stack_height in example_u32_arb(),
            opcode_costs in opcode_costs_arb(),
            host_function_costs in host_function_costs_arb(),
        ) -> WasmV1Config {
            WasmV1Config {
                max_memory,
                max_stack_height,
                opcode_costs,
                host_function_costs,
            }
        }
    }
}
