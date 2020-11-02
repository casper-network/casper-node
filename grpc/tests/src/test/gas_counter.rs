use assert_matches::assert_matches;
use parity_wasm::{
    builder,
    elements::{Instruction, Instructions},
};

use casper_engine_test_support::{
    internal::{
        DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, ARG_AMOUNT,
        DEFAULT_PAYMENT, DEFAULT_RUN_GENESIS_REQUEST, DEFAULT_WASM_CONFIG,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::{
    core::engine_state::Error,
    shared::{gas::Gas, wasm_prep::PreprocessingError},
};
use casper_types::{contracts::DEFAULT_ENTRY_POINT_NAME, runtime_args, RuntimeArgs};

const DO_NOTHING_CONTRACT: &str = "do_nothing.wasm";

/// Prepare malicious payload with amount of opcodes that could potentially overflow injected gas
/// counter.
fn make_gas_counter_overflow() -> Vec<u8> {
    let opcode_costs = DEFAULT_WASM_CONFIG.opcode_costs();

    // Create a lot of `nop` opcodes to potentially overflow gas injector's batching counter.
    let upper_bound = (u32::max_value() as usize / opcode_costs.nop as usize) + 1;

    let instructions = {
        let mut instructions = Vec::with_capacity(upper_bound);
        for _ in 0..upper_bound {
            instructions.push(Instruction::Nop);
        }
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

/// Creates session code with opcodes
fn make_session_code_with(instructions: Vec<Instruction>) -> Vec<u8> {
    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
        // Generated instructions for our entrypoint
        .with_instructions(Instructions::new(instructions))
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

#[ignore]
#[test]
fn should_fail_to_overflow_gas_counter() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let session_bytes = make_gas_counter_overflow();

    let exec_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_bytes(session_bytes, RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request).commit();

    let responses = builder.get_exec_response(0).expect("should have response");
    let response = responses.get(0).expect("should have first element");

    let lhs = response.as_error().expect("should have error");
    assert_matches!(
        lhs,
        Error::WasmPreprocessing(PreprocessingError::OperationForbiddenByGasRules)
    );
}

#[ignore]
#[test]
fn should_correctly_measure_gas_for_opcodes() {
    let opcode_costs = DEFAULT_WASM_CONFIG.opcode_costs();

    const GROW_PAGES: u32 = 1;

    let opcodes = vec![
        (Some(opcode_costs.nop), Instruction::Nop),
        (
            Some(opcode_costs.current_memory),
            Instruction::CurrentMemory(0),
        ), // Push size to stack
        (Some(opcode_costs.op_const), Instruction::I32Const(10)),
        (Some(opcode_costs.mul), Instruction::I32Mul), // memory.size * 10
        (Some(opcode_costs.op_const), Instruction::I32Const(11)),
        (Some(opcode_costs.add), Instruction::I32Add),
        (Some(opcode_costs.op_const), Instruction::I32Const(12)),
        (Some(opcode_costs.add), Instruction::I32Sub),
        (Some(opcode_costs.op_const), Instruction::I32Const(13)),
        (Some(opcode_costs.div), Instruction::I32DivU),
        (Some(opcode_costs.op_const), Instruction::I32Const(3)),
        (Some(opcode_costs.bit), Instruction::I32Shl), // x<<3 == x*(2*3)
        // Store computation
        (Some(opcode_costs.op_const), Instruction::I32Const(0)), // offset
        (Some(opcode_costs.store), Instruction::I32Store(0, 4)), /* Store `memory.size * 10` on
                                                                  * the heap */
        // Grow by N pages
        (
            Some(opcode_costs.op_const),
            Instruction::I32Const(GROW_PAGES as i32),
        ),
        // memory.grow is metered by the number of pages
        (
            Some(opcode_costs.grow_memory * (GROW_PAGES + 1)),
            Instruction::GrowMemory(0),
        ),
        (Some(opcode_costs.op_const), Instruction::I32Const(0)),
        (Some(opcode_costs.store), Instruction::I32Store(0, 4)), /* Store `grow_memory` result
                                                                  * whatever it is */
        // Sentinel instruction that's required to be present but it's not accounted for
        (None, Instruction::End),
    ];

    let instructions = opcodes.iter().map(|(_, instr)| instr.clone()).collect();
    let accounted_opcodes: Vec<_> = opcodes.iter().filter_map(|(cost, _)| *cost).collect();

    let session_bytes = make_session_code_with(instructions);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        DO_NOTHING_CONTRACT,
        RuntimeArgs::new(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request).commit().expect_success();

    let payment_cost = builder.last_exec_gas_cost();

    let exec_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_bytes(session_bytes, RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder.exec(exec_request).commit().expect_success();

    let gas_cost = builder.last_exec_gas_cost() - payment_cost;
    let expected_cost = accounted_opcodes.clone().into_iter().map(Gas::from).sum();
    assert_eq!(
        gas_cost, expected_cost,
        "accounted costs {:?}",
        accounted_opcodes
    );
}
