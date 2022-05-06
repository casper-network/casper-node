use assert_matches::assert_matches;
use parity_wasm::{
    builder,
    elements::{BlockType, Instruction, Instructions},
};

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, ARG_AMOUNT,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT, DEFAULT_WASM_CONFIG, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{core::engine_state::Error, shared::wasm_prep::PreprocessingError};
use casper_types::{contracts::DEFAULT_ENTRY_POINT_NAME, runtime_args, Gas, RuntimeArgs};

/// Prepare malicious payload with amount of opcodes that could potentially overflow injected gas
/// counter.
fn make_gas_counter_overflow() -> Vec<u8> {
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

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    builder.exec(exec_request).commit();

    let responses = builder.get_exec_result(0).expect("should have response");
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

    // A vector of expected cost of given WASM instruction.
    // First element of the tuple represents and option where Some case represents metered
    // instruction and None an instruction that's not accounted for.
    //
    // The idea here is to execute hand written WASM and compare the execution result's gas counter
    // with the expected gathered from here.
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
        // if 0 { nop } else { nop; nop; }
        (Some(opcode_costs.op_const), Instruction::I32Const(0)),
        (
            Some(opcode_costs.control_flow),
            Instruction::If(BlockType::NoResult),
        ),
        (None, Instruction::Nop),
        (None, Instruction::Else),
        // else clause is accounted for only
        (Some(opcode_costs.nop), Instruction::Nop),
        (Some(opcode_costs.nop), Instruction::Nop),
        (None, Instruction::End),
        // 0 == 1
        (Some(opcode_costs.op_const), Instruction::I32Const(0)),
        (Some(opcode_costs.op_const), Instruction::I32Const(1)),
        (Some(opcode_costs.integer_comparison), Instruction::I32Eqz),
        (Some(opcode_costs.store), Instruction::I32Store(0, 4)), /* Store `eqz` result
                                                                  * whatever it is */
        // i32 -> i64
        (Some(opcode_costs.op_const), Instruction::I32Const(123)),
        (Some(opcode_costs.conversion), Instruction::I64ExtendSI32),
        (Some(opcode_costs.control_flow), Instruction::Drop), /* Discard the result */
        // Sentinel instruction that's required to be present but it's not accounted for
        (None, Instruction::End),
    ];

    let instructions = opcodes.iter().map(|(_, instr)| instr.clone()).collect();
    let accounted_opcodes: Vec<_> = opcodes.iter().filter_map(|(cost, _)| *cost).collect();

    let session_bytes = make_session_code_with(instructions);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

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

    let gas_cost = builder.last_exec_gas_cost();
    let expected_cost = accounted_opcodes.clone().into_iter().map(Gas::from).sum();
    assert_eq!(
        gas_cost, expected_cost,
        "accounted costs {:?}",
        accounted_opcodes
    );
}
