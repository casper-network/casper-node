use std::fmt::{self, Display, Formatter};

use parity_wasm::elements::{self, Module};
use pwasm_utils::{self, stack_height};
use thiserror::Error;

use super::wasm_config::WasmConfig;

#[derive(Debug, Clone, Error)]
pub enum PreprocessingError {
    Deserialize(String),
    OperationForbiddenByGasRules,
    StackLimiter,
}

impl From<elements::Error> for PreprocessingError {
    fn from(error: elements::Error) -> Self {
        PreprocessingError::Deserialize(error.to_string())
    }
}

impl Display for PreprocessingError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            PreprocessingError::Deserialize(error) => write!(f, "Deserialization error: {}", error),
            PreprocessingError::OperationForbiddenByGasRules => write!(f, "Encountered operation forbidden by gas rules. Consult instruction -> metering config map"),
            PreprocessingError::StackLimiter => write!(f, "Stack limiter error"),
        }
    }
}

pub struct Preprocessor {
    wasm_config: WasmConfig,
}

impl Preprocessor {
    pub fn new(wasm_config: WasmConfig) -> Self {
        Self { wasm_config }
    }

    pub fn preprocess(&self, module_bytes: &[u8]) -> Result<Module, PreprocessingError> {
        let module = deserialize(module_bytes)?;
        let module = pwasm_utils::externalize_mem(module, None, self.wasm_config.initial_mem);
        let module =
            pwasm_utils::inject_gas_counter(module, &self.wasm_config.opcode_costs.to_set())
                .map_err(|_| PreprocessingError::OperationForbiddenByGasRules)?;
        let module = stack_height::inject_limiter(module, self.wasm_config.max_stack_height)
            .map_err(|_| PreprocessingError::StackLimiter)?;
        Ok(module)
    }
}

// Returns a parity Module from bytes without making modifications or limits
pub fn deserialize(module_bytes: &[u8]) -> Result<Module, PreprocessingError> {
    parity_wasm::deserialize_buffer::<Module>(module_bytes).map_err(Into::into)
}
