use casper_types::contracts::DEFAULT_ENTRY_POINT_NAME;
use parity_wasm::{
    builder,
    elements::{Instruction, Instructions},
};

use casper_engine_test_support::DEFAULT_WASM_CONFIG;

/// Prepare malicious payload with amount of opcodes that could potentially overflow injected gas
/// counter.
pub(crate) fn make_gas_counter_overflow() -> Vec<u8> {
    let opcode_costs = DEFAULT_WASM_CONFIG.opcode_costs();

    // Create a lot of `nop` opcodes to potentially overflow gas injector's batching counter.
    let upper_bound = (u32::max_value() as usize / opcode_costs.nop as usize) + 1;

    let instructions = {
        let mut instructions = vec![Instruction::Nop; upper_bound];
        instructions.push(Instruction::End);
        Instructions::new(instructions)
    };

    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
        // Generated instructions for our entrypoint
        .with_instructions(instructions)
        .build()
        .build()
        // Export above function
        .export()
        .field(DEFAULT_ENTRY_POINT_NAME)
        .build()
        // Memory section is mandatory
        .memory()
        .build()
        .build();
    parity_wasm::serialize(module).expect("should serialize")
}

/// Prepare malicious payload in a form of a wasm module without memory section.
pub(crate) fn make_module_without_memory_section() -> Vec<u8> {
    // Create some opcodes.
    let upper_bound = 10;

    let instructions = {
        let mut instructions = vec![Instruction::Nop; upper_bound];
        instructions.push(Instruction::End);
        Instructions::new(instructions)
    };

    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
        // Generated instructions for our entrypoint
        .with_instructions(instructions)
        .build()
        .build()
        // Export above function
        .export()
        .field(DEFAULT_ENTRY_POINT_NAME)
        .build()
        .build();
    parity_wasm::serialize(module).expect("should serialize")
}

/// Prepare malicious payload in a form of a wasm module with forbidden start section.
pub(crate) fn make_module_with_start_section() -> Vec<u8> {
    let module = r#"
        (module
            (memory 1)
            (start 0)
            (func (export "call")
            )
        )
    "#;
    wabt::wat2wasm(module).expect("should parse wat")
}
