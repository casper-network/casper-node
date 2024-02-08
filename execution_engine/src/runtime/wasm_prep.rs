//! Preprocessing of Wasm modules.
use std::{convert::TryInto, num::NonZeroU32};

use thiserror::Error;

use casper_types::{OpcodeCosts, WasmConfig};
use casper_wasm::elements::{
    self, External, Instruction, Internal, MemorySection, Module, Section, TableType, Type,
};
use casper_wasm_utils::{
    self,
    rules::{MemoryGrowCost, Rules},
    stack_height,
};

use crate::execution;

const DEFAULT_GAS_MODULE_NAME: &str = "env";
/// Name of the internal gas function injected by [`casper_wasm_utils::inject_gas_counter`].
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
pub enum WasmValidationError {
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
#[non_exhaustive]
pub enum PreprocessingError {
    /// Unable to deserialize Wasm bytes.
    #[error("Deserialization error: {0}")]
    Deserialize(String),
    /// Found opcodes forbidden by gas rules.
    #[error(
        "Encountered operation forbidden by gas rules. Consult instruction -> metering config map"
    )]
    OperationForbiddenByGasRules,
    /// Stack limiter was unable to instrument the binary.
    #[error("Stack limiter error")]
    StackLimiter,
    /// Wasm bytes is missing memory section.
    #[error("Memory section should exist")]
    MissingMemorySection,
    /// The module is missing.
    #[error("Missing module")]
    MissingModule,
    /// Unable to validate wasm bytes.
    #[error("Wasm validation error: {0}")]
    WasmValidation(#[from] WasmValidationError),
}

impl From<elements::Error> for PreprocessingError {
    fn from(error: elements::Error) -> Self {
        PreprocessingError::Deserialize(error.to_string())
    }
}

/// Ensures that all the references to functions and global variables in the wasm bytecode are
/// properly declared.
///
/// This validates that:
///
/// - Start function points to a function declared in the Wasm bytecode
/// - All exported functions are pointing to functions declared in the Wasm bytecode
/// - `call` instructions reference a function declared in the Wasm bytecode.
/// - `global.set`, `global.get` instructions are referencing an existing global declared in the
///   Wasm bytecode.
/// - All members of the "elem" section point at functions declared in the Wasm bytecode.
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

fn ensure_valid_function_index(index: u32, function_count: u32) -> Result<(), WasmValidationError> {
    if index >= function_count {
        return Err(WasmValidationError::MissingFunctionIndex { index });
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
pub(crate) fn preprocess(
    wasm_config: WasmConfig,
    module_bytes: &[u8],
) -> Result<Module, PreprocessingError> {
    let module = deserialize(module_bytes)?;

    ensure_valid_access(&module)?;

    if memory_section(&module).is_none() {
        // `casper_wasm_utils::externalize_mem` expects a non-empty memory section to exist in the
        // module, and panics otherwise.
        return Err(PreprocessingError::MissingMemorySection);
    }

    let module = ensure_table_size_limit(module, DEFAULT_MAX_TABLE_SIZE)?;
    ensure_br_table_size_limit(&module, DEFAULT_BR_TABLE_MAX_SIZE)?;
    ensure_global_variable_limit(&module, DEFAULT_MAX_GLOBALS)?;
    ensure_parameter_limit(&module, DEFAULT_MAX_PARAMETER_COUNT)?;
    ensure_valid_imports(&module)?;

    let costs = RuledOpcodeCosts(wasm_config.opcode_costs());
    let module = casper_wasm_utils::externalize_mem(module, None, wasm_config.max_memory);
    let module = casper_wasm_utils::inject_gas_counter(module, &costs, DEFAULT_GAS_MODULE_NAME)
        .map_err(|_| PreprocessingError::OperationForbiddenByGasRules)?;
    let module = stack_height::inject_limiter(module, wasm_config.max_stack_height)
        .map_err(|_| PreprocessingError::StackLimiter)?;
    Ok(module)
}

/// Returns a parity Module from the given bytes without making modifications or checking limits.
pub fn deserialize(module_bytes: &[u8]) -> Result<Module, PreprocessingError> {
    casper_wasm::deserialize_buffer::<Module>(module_bytes).map_err(Into::into)
}

/// Creates new wasm module from entry points.
pub fn get_module_from_entry_points(
    entry_point_names: Vec<&str>,
    mut module: Module,
) -> Result<Vec<u8>, execution::Error> {
    let export_section = module.export_section().ok_or_else(|| {
        execution::Error::FunctionNotFound(String::from("Missing Export Section"))
    })?;

    let maybe_missing_name: Option<String> = entry_point_names
        .iter()
        .find(|name| {
            !export_section
                .entries()
                .iter()
                .any(|export_entry| export_entry.field() == **name)
        })
        .map(|s| String::from(*s));

    match maybe_missing_name {
        Some(missing_name) => Err(execution::Error::FunctionNotFound(missing_name)),
        None => {
            casper_wasm_utils::optimize(&mut module, entry_point_names)?;
            casper_wasm::serialize(module).map_err(execution::Error::ParityWasm)
        }
    }
}

struct RuledOpcodeCosts(OpcodeCosts);

impl Rules for RuledOpcodeCosts {
    fn instruction_cost(&self, instruction: &Instruction) -> Option<u32> {
        let costs = self.0;
        match instruction {
            Instruction::Unreachable => Some(costs.unreachable),
            Instruction::Nop => Some(costs.nop),

            // Control flow class of opcodes is charged for each of the opcode individually.
            Instruction::Block(_) => Some(costs.control_flow.block),
            Instruction::Loop(_) => Some(costs.control_flow.op_loop),
            Instruction::If(_) => Some(costs.control_flow.op_if),
            Instruction::Else => Some(costs.control_flow.op_else),
            Instruction::End => Some(costs.control_flow.end),
            Instruction::Br(_) => Some(costs.control_flow.br),
            Instruction::BrIf(_) => Some(costs.control_flow.br_if),
            Instruction::BrTable(br_table_data) => {
                // If we're unable to fit table size in `u32` to measure the cost, then such wasm
                // would be rejected. This is unlikely scenario as we impose a limit
                // for the amount of targets a `br_table` opcode can contain.
                let br_table_size: u32 = br_table_data.table.len().try_into().ok()?;

                let br_table_cost = costs.control_flow.br_table.cost;

                let table_size_part =
                    br_table_size.checked_mul(costs.control_flow.br_table.size_multiplier)?;

                let br_table_cost = br_table_cost.checked_add(table_size_part)?;
                Some(br_table_cost)
            }
            Instruction::Return => Some(costs.control_flow.op_return),
            Instruction::Call(_) => Some(costs.control_flow.call),
            Instruction::CallIndirect(_, _) => Some(costs.control_flow.call_indirect),
            Instruction::Drop => Some(costs.control_flow.drop),
            Instruction::Select => Some(costs.control_flow.select),

            Instruction::GetLocal(_) | Instruction::SetLocal(_) | Instruction::TeeLocal(_) => {
                Some(costs.local)
            }
            Instruction::GetGlobal(_) | Instruction::SetGlobal(_) => Some(costs.global),

            Instruction::I32Load(_, _)
            | Instruction::I64Load(_, _)
            | Instruction::F32Load(_, _)
            | Instruction::F64Load(_, _)
            | Instruction::I32Load8S(_, _)
            | Instruction::I32Load8U(_, _)
            | Instruction::I32Load16S(_, _)
            | Instruction::I32Load16U(_, _)
            | Instruction::I64Load8S(_, _)
            | Instruction::I64Load8U(_, _)
            | Instruction::I64Load16S(_, _)
            | Instruction::I64Load16U(_, _)
            | Instruction::I64Load32S(_, _)
            | Instruction::I64Load32U(_, _) => Some(costs.load),

            Instruction::I32Store(_, _)
            | Instruction::I64Store(_, _)
            | Instruction::F32Store(_, _)
            | Instruction::F64Store(_, _)
            | Instruction::I32Store8(_, _)
            | Instruction::I32Store16(_, _)
            | Instruction::I64Store8(_, _)
            | Instruction::I64Store16(_, _)
            | Instruction::I64Store32(_, _) => Some(costs.store),

            Instruction::CurrentMemory(_) => Some(costs.current_memory),
            Instruction::GrowMemory(_) => Some(costs.grow_memory),

            Instruction::I32Const(_) | Instruction::I64Const(_) => Some(costs.op_const),

            Instruction::F32Const(_) | Instruction::F64Const(_) => None, // float_const

            Instruction::I32Eqz
            | Instruction::I32Eq
            | Instruction::I32Ne
            | Instruction::I32LtS
            | Instruction::I32LtU
            | Instruction::I32GtS
            | Instruction::I32GtU
            | Instruction::I32LeS
            | Instruction::I32LeU
            | Instruction::I32GeS
            | Instruction::I32GeU
            | Instruction::I64Eqz
            | Instruction::I64Eq
            | Instruction::I64Ne
            | Instruction::I64LtS
            | Instruction::I64LtU
            | Instruction::I64GtS
            | Instruction::I64GtU
            | Instruction::I64LeS
            | Instruction::I64LeU
            | Instruction::I64GeS
            | Instruction::I64GeU => Some(costs.integer_comparison),

            Instruction::F32Eq
            | Instruction::F32Ne
            | Instruction::F32Lt
            | Instruction::F32Gt
            | Instruction::F32Le
            | Instruction::F32Ge
            | Instruction::F64Eq
            | Instruction::F64Ne
            | Instruction::F64Lt
            | Instruction::F64Gt
            | Instruction::F64Le
            | Instruction::F64Ge => None, // Unsupported comparison operators for floats.

            Instruction::I32Clz | Instruction::I32Ctz | Instruction::I32Popcnt => Some(costs.bit),

            Instruction::I32Add | Instruction::I32Sub => Some(costs.add),

            Instruction::I32Mul => Some(costs.mul),

            Instruction::I32DivS
            | Instruction::I32DivU
            | Instruction::I32RemS
            | Instruction::I32RemU => Some(costs.div),

            Instruction::I32And
            | Instruction::I32Or
            | Instruction::I32Xor
            | Instruction::I32Shl
            | Instruction::I32ShrS
            | Instruction::I32ShrU
            | Instruction::I32Rotl
            | Instruction::I32Rotr
            | Instruction::I64Clz
            | Instruction::I64Ctz
            | Instruction::I64Popcnt => Some(costs.bit),

            Instruction::I64Add | Instruction::I64Sub => Some(costs.add),
            Instruction::I64Mul => Some(costs.mul),

            Instruction::I64DivS
            | Instruction::I64DivU
            | Instruction::I64RemS
            | Instruction::I64RemU => Some(costs.div),

            Instruction::I64And
            | Instruction::I64Or
            | Instruction::I64Xor
            | Instruction::I64Shl
            | Instruction::I64ShrS
            | Instruction::I64ShrU
            | Instruction::I64Rotl
            | Instruction::I64Rotr => Some(costs.bit),

            Instruction::F32Abs
            | Instruction::F32Neg
            | Instruction::F32Ceil
            | Instruction::F32Floor
            | Instruction::F32Trunc
            | Instruction::F32Nearest
            | Instruction::F32Sqrt
            | Instruction::F32Add
            | Instruction::F32Sub
            | Instruction::F32Mul
            | Instruction::F32Div
            | Instruction::F32Min
            | Instruction::F32Max
            | Instruction::F32Copysign
            | Instruction::F64Abs
            | Instruction::F64Neg
            | Instruction::F64Ceil
            | Instruction::F64Floor
            | Instruction::F64Trunc
            | Instruction::F64Nearest
            | Instruction::F64Sqrt
            | Instruction::F64Add
            | Instruction::F64Sub
            | Instruction::F64Mul
            | Instruction::F64Div
            | Instruction::F64Min
            | Instruction::F64Max
            | Instruction::F64Copysign => None, // Unsupported math operators for floats.

            Instruction::I32WrapI64 | Instruction::I64ExtendSI32 | Instruction::I64ExtendUI32 => {
                Some(costs.conversion)
            }

            Instruction::I32TruncSF32
            | Instruction::I32TruncUF32
            | Instruction::I32TruncSF64
            | Instruction::I32TruncUF64
            | Instruction::I64TruncSF32
            | Instruction::I64TruncUF32
            | Instruction::I64TruncSF64
            | Instruction::I64TruncUF64
            | Instruction::F32ConvertSI32
            | Instruction::F32ConvertUI32
            | Instruction::F32ConvertSI64
            | Instruction::F32ConvertUI64
            | Instruction::F32DemoteF64
            | Instruction::F64ConvertSI32
            | Instruction::F64ConvertUI32
            | Instruction::F64ConvertSI64
            | Instruction::F64ConvertUI64
            | Instruction::F64PromoteF32 => None, // Unsupported conversion operators for floats.

            Instruction::I32ReinterpretF32
            | Instruction::I64ReinterpretF64
            | Instruction::F32ReinterpretI32
            | Instruction::F64ReinterpretI64 => None, /* Unsupported reinterpretation operators
                                                       * for floats. */
        }
    }

    fn memory_grow_cost(&self) -> Option<MemoryGrowCost> {
        NonZeroU32::new(self.0.grow_memory).map(MemoryGrowCost::Linear)
    }
}

#[cfg(test)]
mod tests {
    use casper_types::addressable_entity::DEFAULT_ENTRY_POINT_NAME;
    use casper_wasm::{
        builder,
        elements::{CodeSection, Instructions},
    };
    use walrus::{FunctionBuilder, ModuleConfig, ValType};

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
        let module_bytes = casper_wasm::serialize(module).expect("should serialize");
        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(
                &error,
                PreprocessingError::WasmValidation(WasmValidationError::MissingFunctionIndex { index: missing_index })
                if *missing_index == u32::MAX
            ),
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
        let module_bytes = casper_wasm::serialize(module).expect("should serialize");
        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(
                &error,
                PreprocessingError::WasmValidation(WasmValidationError::MissingFunctionIndex { index: missing_index })
                if *missing_index == u32::MAX
            ),
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
        let module_bytes = casper_wasm::serialize(module).expect("should serialize");
        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(
                &error,
                PreprocessingError::WasmValidation(WasmValidationError::MissingFunctionIndex { index: missing_index })
                if *missing_index == u32::MAX
            ),
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
        let module_bytes = casper_wasm::serialize(module).expect("should serialize");

        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(
                &error,
                PreprocessingError::WasmValidation(WasmValidationError::MissingFunctionIndex { index: missing_index })
                if *missing_index == u32::MAX
            ),
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
        let module_bytes = casper_wasm::serialize(module).expect("should serialize");
        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(
                &error,
                PreprocessingError::WasmValidation(WasmValidationError::MissingFunctionIndex { index: missing_index })
                if *missing_index == u32::MAX
            ),
            "{:?}",
            error,
        );
    }

    #[test]
    fn should_not_accept_multi_value_proposal_wasm() {
        let module_bytes = {
            let mut module = walrus::Module::with_config(ModuleConfig::new());

            let _memory_id = module.memories.add_local(false, 11, None);

            let mut func_with_locals =
                FunctionBuilder::new(&mut module.types, &[], &[ValType::I32, ValType::I64]);

            func_with_locals.func_body().i64_const(0).i32_const(1);

            let func_with_locals = func_with_locals.finish(vec![], &mut module.funcs);

            let mut call_func = FunctionBuilder::new(&mut module.types, &[], &[]);

            call_func.func_body().call(func_with_locals);

            let call = call_func.finish(Vec::new(), &mut module.funcs);

            module.exports.add(DEFAULT_ENTRY_POINT_NAME, call);

            module.emit_wasm()
        };
        let error = preprocess(WasmConfig::default(), &module_bytes)
            .expect_err("should fail with an error");
        assert!(
            matches!(&error, PreprocessingError::Deserialize(msg)
            // TODO: GH-3762 will improve the error message for unsupported wasm proposals.
            if msg == "Enable the multi_value feature to deserialize more than one function result"),
            "{:?}",
            error,
        );
    }
}
