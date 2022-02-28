//! Preprocessing of Wasm modules.
use std::fmt::{self, Display, Formatter};

use parity_wasm::elements::{self, MemorySection, Module, Section};
use pwasm_utils::{self, stack_height};
use thiserror::Error;

use super::wasm_config::WasmConfig;

const DEFAULT_GAS_MODULE_NAME: &str = "env";

/// An error emitted by the Wasm preprocessor.
#[derive(Debug, Clone, Error)]
pub enum PreprocessingError {
    /// Unable to deserialize Wasm bytes.
    Deserialize(String),
    /// Found opcodes forbidden by gas rules.
    OperationForbiddenByGasRules,
    /// Stack limiter was unable to instrument the binary.
    StackLimiter,
    /// Wasm bytes is missing memory section.
    MissingMemorySection,
    /// The module is missing.
    MissingModule,
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
            PreprocessingError::MissingMemorySection => write!(f, "Memory section should exist"),
            PreprocessingError::MissingModule => write!(f, "Missing module")
        }
    }
}

/// Checks if given wasm module contains a memory section.
fn memory_section(module: &Module) -> Option<&MemorySection> {
    for section in module.sections() {
        if let Section::Memory(section) = section {
            return Some(section);
        }
    }
    None
}

/// Preprocesses Wasm bytes and returns a module.
///
/// This process consists of a few steps:
/// - Validate that the given bytes contain a memory section, and check the memory page limit.
/// - Inject gas counters into the code, which makes it possible for the executed Wasm to be charged
///   for opcodes; this also validates opcodes and ensures that there are no forbidden opcodes in
///   use, such as floating point opcodes.
/// - Ensure that the code has a maximum stack height.
///
/// In case the preprocessing rules can't be applied, an error is returned.
/// Otherwise, this method returns a valid module ready to be executed safely on the host.
pub fn preprocess(
    wasm_config: WasmConfig,
    module_bytes: &[u8],
) -> Result<Module, PreprocessingError> {
    let module = deserialize(module_bytes)?;

    if memory_section(&module).is_none() {
        // `pwasm_utils::externalize_mem` expects a memory section to exist in the module, and
        // panics otherwise.
        return Err(PreprocessingError::MissingMemorySection);
    }

    let module = pwasm_utils::externalize_mem(module, None, wasm_config.max_memory);
    let module = pwasm_utils::inject_gas_counter(
        module,
        &wasm_config.opcode_costs().to_set(),
        DEFAULT_GAS_MODULE_NAME,
    )
    .map_err(|_| PreprocessingError::OperationForbiddenByGasRules)?;
    let module = stack_height::inject_limiter(module, wasm_config.max_stack_height)
        .map_err(|_| PreprocessingError::StackLimiter)?;
    Ok(module)
}

/// Returns a parity Module from the given bytes without making modifications or checking limits.
pub fn deserialize(module_bytes: &[u8]) -> Result<Module, PreprocessingError> {
    parity_wasm::deserialize_buffer::<Module>(module_bytes).map_err(Into::into)
}
