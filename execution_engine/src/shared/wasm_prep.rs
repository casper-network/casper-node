//! Preprocessing of Wasm modules.
use std::fmt::{self, Display, Formatter};

use parity_wasm::elements::{
    self, External, Instruction, Internal, MemorySection, Module, Section, TableType, Type,
};
use pwasm_utils::{self, stack_height};
use thiserror::Error;

use super::wasm_config::WasmConfig;

const DEFAULT_GAS_MODULE_NAME: &str = "env";
/// Name of the internal gas function injected by [`pwasm_utils::inject_gas_counter`].
const INTERNAL_GAS_FUNCTION_NAME: &str = "gas";

/// We only allow maximum of 4k function pointers in a table section.
pub const DEFAULT_MAX_TABLE_SIZE: u32 = 4096;
/// Maximum number of elements that can appear as immediate value to the br_table instruction.
pub const DEFAULT_BR_TABLE_MAX_SIZE: u32 = 256;
/// Maximum number of global a module is allowed to declare.
pub const DEFAULT_MAX_GLOBALS: u32 = 256;
/// Maximum number of parameters a function can have.
pub const DEFAULT_MAX_PARAMETER_COUNT: u32 = 256;

/// An error emitted by the Wasm preprocessor.
#[derive(Debug, Clone, Error)]
#[non_exhaustive]
enum WasmValidationError {
    /// Initial table size outside allowed bounds.
    #[error("initial table size of {actual} exceeds allowed limit of {max}")]
    InitialTableSizeExceeded {
        /// Allowed maximum table size.
        max: u32,
        /// Actual initial table size specified in the Wasm.
        actual: u32,
    },
    /// Maximum table size outside allowed bounds.
    #[error("maximum table size of {actual} exceeds allowed limit of {max}")]
    MaxTableSizeExceeded {
        /// Allowed maximum table size.
        max: u32,
        /// Actual max table size specified in the Wasm.
        actual: u32,
    },
    /// Number of the tables in a Wasm must be at most one.
    #[error("the number of tables must be at most one")]
    MoreThanOneTable,
    /// Length of a br_table exceeded the maximum allowed size.
    #[error("maximum br_table size of {actual} exceeds allowed limit of {max}")]
    BrTableSizeExceeded {
        /// Maximum allowed br_table length.
        max: u32,
        /// Actual size of a br_table in the code.
        actual: usize,
    },
    /// Declared number of globals exceeds allowed limit.
    #[error("declared number of globals ({actual}) exceeds allowed limit of {max}")]
    TooManyGlobals {
        /// Maximum allowed globals.
        max: u32,
        /// Actual number of globals declared in the Wasm.
        actual: usize,
    },
    /// Module declares a function type with too many parameters.
    #[error("use of a function type with too many parameters (limit of {max} but function declares {actual})")]
    TooManyParameters {
        /// Maximum allowed parameters.
        max: u32,
        /// Actual number of parameters a function has in the Wasm.
        actual: usize,
    },
    /// Module tries to import a function that the host does not provide.
    #[error("module imports a non-existent function")]
    MissingHostFunction,
    /// Opcode for a global access refers to a non-existing global
    #[error("opcode for a global access refers to non-existing global index {index}")]
    IncorrectGlobalOperation {
        /// Provided index.
        index: u32,
    },
    /// Missing function index.
    #[error("missing function index {index}")]
    MissingFunctionIndex {
        /// Provided index.
        index: u32,
    },
    /// Missing function type.
    #[error("missing type index {index}")]
    MissingFunctionType {
        /// Provided index.
        index: u32,
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
}

impl From<elements::Error> for PreprocessingError {
    fn from(error: elements::Error) -> Self {
        PreprocessingError::Deserialize(error.to_string())
    }
}

impl From<WasmValidationError> for PreprocessingError {
    fn from(wasm_validation_error: WasmValidationError) -> Self {
        Self::Deserialize(wasm_validation_error.to_string())
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
        }
    }
}

fn ensure_valid_access(module: &Module) -> Result<(), WasmValidationError> {
    let function_types_count = module
        .type_section()
        .map(|ts| ts.types().len())
        .unwrap_or_default();

    let mut function_count = 0_u32;
    if let Some(import_section) = module.import_section() {
        for import_entry in import_section.entries() {
            if let External::Function(function_type_index) = import_entry.external() {
                if (*function_type_index as usize) < function_types_count {
                    function_count = function_count.saturating_add(1);
                } else {
                    return Err(WasmValidationError::MissingFunctionType {
                        index: *function_type_index,
                    });
                }
            }
        }
    }
    if let Some(function_section) = module.function_section() {
        for function_entry in function_section.entries() {
            let function_type_index = function_entry.type_ref();
            if (function_type_index as usize) < function_types_count {
                function_count = function_count.saturating_add(1);
            } else {
                return Err(WasmValidationError::MissingFunctionType {
                    index: function_type_index,
                });
            }
        }
    }

    if let Some(function_index) = module.start_section() {
        ensure_valid_function_index(function_index, function_count)?;
    }
    if let Some(export_section) = module.export_section() {
        for export_entry in export_section.entries() {
            if let Internal::Function(function_index) = export_entry.internal() {
                ensure_valid_function_index(*function_index, function_count)?;
            }
        }
    }

    if let Some(code_section) = module.code_section() {
        let global_len = module
            .global_section()
            .map(|global_section| global_section.entries().len())
            .unwrap_or(0);

        for instr in code_section
            .bodies()
            .iter()
            .flat_map(|body| body.code().elements())
        {
            match instr {
                Instruction::Call(idx) => {
                    ensure_valid_function_index(*idx, function_count)?;
                }
                Instruction::GetGlobal(idx) | Instruction::SetGlobal(idx)
                    if *idx as usize >= global_len =>
                {
                    return Err(WasmValidationError::IncorrectGlobalOperation { index: *idx });
                }
                _ => {}
            }
        }
    }

    if let Some(element_section) = module.elements_section() {
        for element_segment in element_section.entries() {
            for idx in element_segment.members() {
                ensure_valid_function_index(*idx, function_count)?;
            }
        }
    }

    Ok(())
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
fn ensure_table_size_limit(mut module: Module, limit: u32) -> Result<Module, WasmValidationError> {
    if let Some(sect) = module.table_section_mut() {
        // Table section is optional and there can be at most one.
        if sect.entries().len() > 1 {
            return Err(WasmValidationError::MoreThanOneTable);
        }

        if let Some(table_entry) = sect.entries_mut().first_mut() {
            let initial = table_entry.limits().initial();
            if initial > limit {
                return Err(WasmValidationError::InitialTableSizeExceeded {
                    max: limit,
                    actual: initial,
                });
            }

            match table_entry.limits().maximum() {
                Some(max) => {
                    if max > limit {
                        return Err(WasmValidationError::MaxTableSizeExceeded {
                            max: limit,
                            actual: max,
                        });
                    }
                }
                None => {
                    // rewrite wasm and provide a maximum limit for a table section
                    *table_entry = TableType::new(initial, Some(limit))
                }
            }
        }
    }

    Ok(module)
}

/// Ensure that any `br_table` instruction adheres to its immediate value limit.
fn ensure_br_table_size_limit(module: &Module, limit: u32) -> Result<(), WasmValidationError> {
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
            if br_table_data.table.len() > limit as usize {
                return Err(WasmValidationError::BrTableSizeExceeded {
                    max: limit,
                    actual: br_table_data.table.len(),
                });
            }
        }
    }
    Ok(())
}

/// Ensures that module doesn't declare too many globals.
///
/// Globals are not limited through the `stack_height` as locals are. Neither does
/// the linear memory limit `memory_pages` applies to them.
fn ensure_global_variable_limit(module: &Module, limit: u32) -> Result<(), WasmValidationError> {
    if let Some(global_section) = module.global_section() {
        let actual = global_section.entries().len();
        if actual > limit as usize {
            return Err(WasmValidationError::TooManyGlobals { max: limit, actual });
        }
    }
    Ok(())
}

/// Ensure maximum numbers of parameters a function can have.
///
/// Those need to be limited to prevent a potentially exploitable interaction with
/// the stack height instrumentation: The costs of executing the stack height
/// instrumentation for an indirectly called function scales linearly with the amount
/// of parameters of this function. Because the stack height instrumentation itself is
/// is not weight metered its costs must be static (via this limit) and included in
/// the costs of the instructions that cause them (call, call_indirect).
fn ensure_parameter_limit(module: &Module, limit: u32) -> Result<(), WasmValidationError> {
    let type_section = if let Some(type_section) = module.type_section() {
        type_section
    } else {
        return Ok(());
    };

    for Type::Function(func) in type_section.types() {
        let actual = func.params().len();
        if actual > limit as usize {
            return Err(WasmValidationError::TooManyParameters { max: limit, actual });
        }
    }

    Ok(())
}

/// Ensures that Wasm module has valid imports.
fn ensure_valid_imports(module: &Module) -> Result<(), WasmValidationError> {
    let import_entries = module
        .import_section()
        .map(|is| is.entries())
        .unwrap_or(&[]);

    // Gas counter is currently considered an implementation detail.
    //
    // If a wasm module tries to import it will be rejected.

    for import in import_entries {
        if import.module() == DEFAULT_GAS_MODULE_NAME
            && import.field() == INTERNAL_GAS_FUNCTION_NAME
        {
            return Err(WasmValidationError::MissingHostFunction);
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

    ensure_valid_access(&module)?;

    if memory_section(&module).is_none() {
        // `pwasm_utils::externalize_mem` expects a non-empty memory section to exist in the module,
        // and panics otherwise.
        return Err(PreprocessingError::MissingMemorySection);
    }

    let module = ensure_table_size_limit(module, DEFAULT_MAX_TABLE_SIZE)?;
    ensure_br_table_size_limit(&module, DEFAULT_BR_TABLE_MAX_SIZE)?;
    ensure_global_variable_limit(&module, DEFAULT_MAX_GLOBALS)?;
    ensure_parameter_limit(&module, DEFAULT_MAX_PARAMETER_COUNT)?;
    ensure_valid_imports(&module)?;

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

fn ensure_valid_function_index(index: u32, function_count: u32) -> Result<(), WasmValidationError> {
    if index >= function_count {
        return Err(WasmValidationError::MissingFunctionIndex { index });
    }
    Ok(())
}

/// Returns a parity Module from the given bytes without making modifications or checking limits.
pub fn deserialize(module_bytes: &[u8]) -> Result<Module, PreprocessingError> {
    parity_wasm::deserialize_buffer::<Module>(module_bytes).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use casper_types::contracts::DEFAULT_ENTRY_POINT_NAME;
    use parity_wasm::{
        builder,
        elements::{CodeSection, Instructions},
    };

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

    #[test]
    fn should_not_overflow_in_export_section() {
        let module = builder::module()
            .function()
            .signature()
            .build()
            .body()
            .with_instructions(Instructions::new(vec![Instruction::Nop, Instruction::End]))
            .build()
            .build()
            .export()
            .field(DEFAULT_ENTRY_POINT_NAME)
            .internal()
            .func(u32::MAX)
            .build()
            // Memory section is mandatory
            .memory()
            .build()
            .build();
        let module_bytes = parity_wasm::serialize(module).expect("should serialize");
        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(&error, PreprocessingError::Deserialize(_msg)),
            "{:?}",
            error,
        );
    }

    #[test]
    fn should_not_overflow_in_element_section() {
        const CALL_FN_IDX: u32 = 0;

        let module = builder::module()
            .function()
            .signature()
            .build()
            .body()
            .with_instructions(Instructions::new(vec![Instruction::Nop, Instruction::End]))
            .build()
            .build()
            // Export above function
            .export()
            .field(DEFAULT_ENTRY_POINT_NAME)
            .internal()
            .func(CALL_FN_IDX)
            .build()
            .table()
            .with_element(u32::MAX, vec![u32::MAX])
            .build()
            // Memory section is mandatory
            .memory()
            .build()
            .build();
        let module_bytes = parity_wasm::serialize(module).expect("should serialize");
        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(&error, PreprocessingError::Deserialize(_msg)),
            "{:?}",
            error,
        );
    }

    #[test]
    fn should_not_overflow_in_call_opcode() {
        let module = builder::module()
            .function()
            .signature()
            .build()
            .body()
            .with_instructions(Instructions::new(vec![
                Instruction::Call(u32::MAX),
                Instruction::End,
            ]))
            .build()
            .build()
            // Export above function
            .export()
            .field(DEFAULT_ENTRY_POINT_NAME)
            .build()
            // .with_sections(vec![Section::Start(u32::MAX)])
            // Memory section is mandatory
            .memory()
            .build()
            .build();
        let module_bytes = parity_wasm::serialize(module).expect("should serialize");
        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(&error, PreprocessingError::Deserialize(msg)
            if msg == &format!("missing function index {index}", index=u32::MAX)),
            "{:?}",
            error,
        );
    }

    #[test]
    fn should_not_overflow_in_start_section_without_code_section() {
        let module = builder::module()
            .with_section(Section::Start(u32::MAX))
            .memory()
            .build()
            .build();
        let module_bytes = parity_wasm::serialize(module).expect("should serialize");

        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(&error, PreprocessingError::Deserialize(msg)
            if msg == &format!("missing function index {index}", index=u32::MAX)),
            "{:?}",
            error,
        );
    }

    #[test]
    fn should_not_overflow_in_start_section_with_code() {
        let module = builder::module()
            .with_section(Section::Start(u32::MAX))
            .with_section(Section::Code(CodeSection::with_bodies(Vec::new())))
            .memory()
            .build()
            .build();
        let module_bytes = parity_wasm::serialize(module).expect("should serialize");
        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(&error, PreprocessingError::Deserialize(msg)
            if msg == &format!("missing function index {index}", index=u32::MAX)),
            "{:?}",
            error,
        );
    }
}
