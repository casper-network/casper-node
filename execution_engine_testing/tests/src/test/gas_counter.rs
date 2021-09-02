use assert_matches::assert_matches;
use num_traits::{CheckedAdd, Zero};
use parity_wasm::{
    builder,
    elements::{BlockType, Instruction, Instructions},
};

use casper_engine_test_support::{
    internal::{
        DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, ARG_AMOUNT,
        DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_AUCTION_DELAY, DEFAULT_GENESIS_CONFIG_HASH,
        DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PAYMENT,
        DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION, DEFAULT_ROUND_SEIGNIORAGE_RATE,
        DEFAULT_RUN_GENESIS_REQUEST, DEFAULT_SYSTEM_CONFIG, DEFAULT_UNBONDING_DELAY,
        DEFAULT_VALIDATOR_SLOTS, DEFAULT_WASM_CONFIG,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};
use casper_execution_engine::{
    core::engine_state::{
        run_genesis_request::RunGenesisRequest, Error, ExecConfig, GenesisAccount,
    },
    shared::wasm_prep::PreprocessingError,
};
use casper_types::{
    contracts::DEFAULT_ENTRY_POINT_NAME, runtime_args, system::mint, Gas, Motes, RuntimeArgs, U512,
};

const ARG_PURSE_NAME: &str = "purse_name";
const NEW_PURSE: &str = "new_purse";
const NAMED_PURSE_PAYMENT_CONTRACT: &str = "named_purse_payment.wasm";
const CREATE_PURSE_01_CONTRACT: &str = "create_purse_01.wasm";
const GET_ARG_CONTRACT: &str = "get_arg.wasm";
const ARG_VALUE0: &str = "value0";
const ARG_VALUE1: &str = "value1";

/// Creates minimal session code that does nothing
fn make_minimal_do_nothing() -> Vec<u8> {
    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
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

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

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

    let exec_request = {
        // NOTE: We use computed "do nothing" WASM module because it turns out "do_nothing" in
        // AssemblyScript actually does "nop" which really "does something": (func (;10;)
        // (type 4)   nop)

        let do_nothing_bytes = make_minimal_do_nothing();

        let deploy_item = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_bytes(do_nothing_bytes, RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([43; 32])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let payment_cost = {
        let mut forked_builder = builder.clone();
        forked_builder.exec(exec_request).commit().expect_success();
        forked_builder.last_exec_gas_cost()
    };

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
    let expected_cost = accounted_opcodes
        .clone()
        .into_iter()
        .map(Gas::from)
        .try_fold(Gas::zero(), |acc, x| acc.checked_add(&x))
        .expect("should sum without overflow");

    assert_eq!(
        gas_cost, expected_cost,
        "accounted costs {:?}",
        accounted_opcodes
    );
}

#[ignore]
#[test]
fn should_not_fail_with_payment_amount_larger_than_u64_max() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let genesis_request = {
        let accounts = {
            let mut ret = Vec::new();
            let genesis_account =
                GenesisAccount::account(DEFAULT_ACCOUNT_PUBLIC_KEY.clone(), Motes::MAX, None);
            ret.push(genesis_account);
            let proposer_account = GenesisAccount::account(
                DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
                Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE)),
                None,
            );
            ret.push(proposer_account);
            ret
        };
        let exec_config = ExecConfig::new(
            accounts,
            *DEFAULT_WASM_CONFIG,
            *DEFAULT_SYSTEM_CONFIG,
            DEFAULT_VALIDATOR_SLOTS,
            DEFAULT_AUCTION_DELAY,
            DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
            DEFAULT_ROUND_SEIGNIORAGE_RATE,
            DEFAULT_UNBONDING_DELAY,
            DEFAULT_GENESIS_TIMESTAMP_MILLIS,
        );

        RunGenesisRequest::new(
            *DEFAULT_GENESIS_CONFIG_HASH,
            *DEFAULT_PROTOCOL_VERSION,
            exec_config,
        )
    };

    builder.run_genesis(&genesis_request);

    let create_purse_request = {
        ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CREATE_PURSE_01_CONTRACT,
            runtime_args! {
                ARG_PURSE_NAME => NEW_PURSE,
            },
        )
        .build()
    };

    builder.exec(create_purse_request).expect_success().commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let new_purse = account
        .named_keys()
        .get(NEW_PURSE)
        .cloned()
        .expect("should have new purse")
        .into_uref()
        .expect("should be uref");

    let large_amount = U512::from(u128::MAX);

    let transfer_request = {
        ExecuteRequestBuilder::transfer(
            *DEFAULT_ACCOUNT_ADDR,
            runtime_args! {
                mint::ARG_SOURCE => account.main_purse(),
                mint::ARG_TARGET => new_purse,
                mint::ARG_AMOUNT => large_amount,
                mint::ARG_ID => <Option<u64>>::None,
            },
        )
        .build()
    };

    builder.exec(transfer_request).expect_success().commit();

    let payment_amount = U512::from(u64::MAX) + U512::from(1);

    let exec_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(
                GET_ARG_CONTRACT,
                runtime_args! {
                    ARG_VALUE0 => "Hello, world!".to_string(),
                    ARG_VALUE1 => U512::from(42),
                },
            )
            .with_payment_code(
                NAMED_PURSE_PAYMENT_CONTRACT,
                runtime_args! {
                    ARG_PURSE_NAME => NEW_PURSE,
                    ARG_AMOUNT => payment_amount,
                },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .with_gas_price(1)
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder.exec(exec_request).commit().expect_success();

    assert_ne!(builder.last_exec_gas_cost(), Gas::zero());
}
