use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    chainspec::vm_config::OpcodeCosts,
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use super::HostFFIFunctionCosts;

/// Default maximum number of pages of the Wasm memory.
pub const DEFAULT_V2_WASM_MAX_MEMORY: u32 = 64;

/// Configuration of the Wasm execution environment for V2 execution machine.
///
/// This structure contains various Wasm execution configuration options, such as memory limits and
/// costs.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct WasmV2Config {
    /// Maximum amount of heap memory each contract can use.
    max_memory: u32,
    /// Wasm opcode costs table.
    opcode_costs: OpcodeCosts,
    /// Host function costs table.
    host_ffi_opt_costs: HostFFIFunctionCosts,
}

impl WasmV2Config {
    /// ctor
    pub fn new(
        max_memory: u32,
        opcode_costs: OpcodeCosts,
        host_ffi_opt_costs: HostFFIFunctionCosts,
    ) -> Self {
        WasmV2Config {
            max_memory,
            opcode_costs,
            host_ffi_opt_costs,
        }
    }

    /// Returns opcode costs.
    pub fn opcode_costs(&self) -> OpcodeCosts {
        self.opcode_costs
    }

    /// Returns a reference to host function costs
    pub fn host_ffi_opt_costs(&self) -> &HostFFIFunctionCosts {
        &self.host_ffi_opt_costs
    }

    /// Returns host function costs and consumes this object.
    pub fn take_host_ffi_opt_costs(self) -> HostFFIFunctionCosts {
        self.host_ffi_opt_costs
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
}

impl Default for WasmV2Config {
    fn default() -> Self {
        Self {
            max_memory: DEFAULT_V2_WASM_MAX_MEMORY,
            opcode_costs: OpcodeCosts::default(),
            host_ffi_opt_costs: HostFFIFunctionCosts::default(),
        }
    }
}

impl ToBytes for WasmV2Config {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.max_memory.to_bytes()?);
        ret.append(&mut self.opcode_costs.to_bytes()?);
        ret.append(&mut self.host_ffi_opt_costs.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.max_memory.serialized_length()
            + self.opcode_costs.serialized_length()
            + self.host_ffi_opt_costs.serialized_length()
    }
}

impl FromBytes for WasmV2Config {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_memory, rem) = FromBytes::from_bytes(bytes)?;
        let (opcode_costs, rem) = FromBytes::from_bytes(rem)?;
        let (host_ffi_opt_costs, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            WasmV2Config {
                max_memory,
                opcode_costs,
                host_ffi_opt_costs,
            },
            rem,
        ))
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<WasmV2Config> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> WasmV2Config {
        WasmV2Config {
            max_memory: rng.gen(),
            opcode_costs: rng.gen(),
            host_ffi_opt_costs: rng.gen(),
        }
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use crate::{
        chainspec::vm_config::{
            host_ffi_function_costs::gens::host_ffi_opt_costs_arb,
            opcode_costs::gens::opcode_costs_arb,
        },
        gens::example_u32_arb,
    };
    use proptest::prop_compose;

    use super::WasmV2Config;

    prop_compose! {
        pub fn wasm_v2_config_arb() (
            max_memory in example_u32_arb(),
            opcode_costs in opcode_costs_arb(),
            host_ffi_opt_costs in host_ffi_opt_costs_arb(),
        ) -> WasmV2Config {
            WasmV2Config {
                max_memory,
                opcode_costs,
                host_ffi_opt_costs,
            }
        }
    }
}
