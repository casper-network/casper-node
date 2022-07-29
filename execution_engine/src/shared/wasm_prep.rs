//! Preprocessing of Wasm modules.
use std::fmt::{self, Display, Formatter};

use parity_wasm::elements::{self, Instruction, MemorySection, Module, Section, TableType};
use pwasm_utils::{self, stack_height};
use thiserror::Error;

use super::wasm_config::WasmConfig;

const DEFAULT_GAS_MODULE_NAME: &str = "env";
/// We only allow maximum of 4k function pointers in a table section.
pub const DEFAULT_MAX_TABLE_SIZE: u32 = 4096;
/// Maximum number of elements that can appear as immediate value to the br_table instruction.
pub const DEFAULT_BR_TABLE_MAX_SIZE: u32 = 256;

/// An error emitted by the Wasm preprocessor.
#[derive(Debug, Clone, Error)]
pub enum WasmValidationError {
    /// Initial table size outside allowed bounds.
    #[error("initial table size exceeds allowed bounds")]
    InitialTableSizeExceeded {
        /// Allowed maximum table size.
        max: u32,
        /// Actual maximum table size in the Wasm.
        actual: u32,
    },
    /// Maximum table size outside allowed bounds.
    #[error("maximum table size outside allowed bounds")]
    MaxTableSizeExceeded {
        /// Allowed maximum initial table size.
        max: u32,
        /// Actual initial table szie in the Wasm.
        actual: u32,
    },
    /// Number of the tables in a Wasm must be at most one.
    #[error("the number of tables must be at most one")]
    MoreThanOneTable,
    /// Length of a br_table exceeded the maximum allowed size.
    #[error("maximum br_table size exceeds allowed bounds (expected {max} but found {actual})")]
    BrTableSizeExceeded {
        /// Maximum allowed br_table length.
        max: u32,
        /// Actual size of the largest br_table in the code.
        actual: usize,
    },
}

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
    /// Wasm validation did not pass.
    InvalidWasm(#[from] WasmValidationError),
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
            PreprocessingError::MissingModule => write!(f, "Missing module"),
            PreprocessingError::InvalidWasm(msg) => write!(f, "Invalid wasm: {msg}."),

        }
    }
}

/// Checks if given wasm module contains a non-empty memory section.
fn memory_section(module: &Module) -> Option<&MemorySection> {
    for section in module.sections() {
        if let Section::Memory(section) = section {
            return if section.entries().is_empty() {
                None
            } else {
                Some(section)
            };
        }
    }
    None
}

/// Ensures (table) section has at most one table entry, and initial, and maximum values are
/// normalized.
///
/// If a maximum value is not specified it will be defaulted to 4k to prevent OOM.
fn ensure_table_size_limit(mut module: Module) -> Result<Module, WasmValidationError> {
    if let Some(sect) = module.table_section_mut() {
        // Table section is optional and there can be at most one.
        if sect.entries().len() > 1 {
            return Err(WasmValidationError::MoreThanOneTable);
        }

        if let Some(table_entry) = sect.entries_mut().iter_mut().next() {
            let initial = table_entry.limits().initial();
            if initial > DEFAULT_MAX_TABLE_SIZE {
                return Err(WasmValidationError::InitialTableSizeExceeded {
                    max: DEFAULT_MAX_TABLE_SIZE,
                    actual: initial,
                });
            }

            match table_entry.limits().maximum() {
                Some(max) if max > DEFAULT_MAX_TABLE_SIZE => {
                    return Err(WasmValidationError::MaxTableSizeExceeded {
                        max: DEFAULT_MAX_TABLE_SIZE,
                        actual: max,
                    })
                }
                Some(_) => {
                    // maximum within the limit
                }
                None => {
                    // rewrite wasm and provide a maximum limit for a table section
                    *table_entry = TableType::new(initial, Some(DEFAULT_MAX_TABLE_SIZE))
                }
            }
        }
    }

    Ok(module)
}

/// Ensure that any `br_table` instruction adheres to its immediate value limit.
fn ensure_br_table_size_limit(module: &Module) -> Result<(), WasmValidationError> {
    let code_section = if let Some(type_section) = module.code_section() {
        type_section
    } else {
        return Ok(());
    };
    for instr in code_section
        .bodies()
        .iter()
        .flat_map(|body| body.code().elements())
    {
        if let Instruction::BrTable(br_table_data) = instr {
            if br_table_data.table.len() > DEFAULT_BR_TABLE_MAX_SIZE as usize {
                return Err(WasmValidationError::BrTableSizeExceeded {
                    max: DEFAULT_BR_TABLE_MAX_SIZE,
                    actual: br_table_data.table.len(),
                });
            }
        }
    }
    Ok(())
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
        // `pwasm_utils::externalize_mem` expects a non-empty memory section to exist in the module,
        // and panics otherwise.
        return Err(PreprocessingError::MissingMemorySection);
    }

    let module = ensure_table_size_limit(module)?;
    ensure_br_table_size_limit(&module)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_not_panic_on_empty_memory() {
        // These bytes were generated during fuzz testing and are compiled from Wasm which
        // deserializes to a `Module` with a memory section containing no entries.
        const MODULE_BYTES_WITH_EMPTY_MEMORY: [u8; 61] = [
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x09, 0x02, 0x60, 0x01, 0x7f,
            0x01, 0x7f, 0x60, 0x00, 0x00, 0x03, 0x03, 0x02, 0x00, 0x01, 0x05, 0x01, 0x00, 0x08,
            0x01, 0x01, 0x0a, 0x1d, 0x02, 0x18, 0x00, 0x20, 0x00, 0x41, 0x80, 0x80, 0x82, 0x80,
            0x78, 0x70, 0x41, 0x80, 0x82, 0x80, 0x80, 0x7e, 0x4f, 0x22, 0x00, 0x1a, 0x20, 0x00,
            0x0f, 0x0b, 0x02, 0x00, 0x0b,
        ];

        match preprocess(WasmConfig::default(), &MODULE_BYTES_WITH_EMPTY_MEMORY).unwrap_err() {
            PreprocessingError::MissingMemorySection => (),
            error => panic!("expected MissingMemorySection, got {:?}", error),
        }
    }
}
