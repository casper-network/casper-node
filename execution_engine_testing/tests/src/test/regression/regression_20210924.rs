use num_traits::Zero;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    core::engine_state::{Error as CoreError, MAX_PAYMENT},
    shared::opcode_costs::DEFAULT_NOP_COST,
};
use casper_types::{contracts::DEFAULT_ENTRY_POINT_NAME, runtime_args, Gas, RuntimeArgs, U512};
use parity_wasm::{
    builder,
    elements::{Instruction, Instructions},
};

use crate::wasm_utils;

const ARG_AMOUNT: &str = "amount";

/// Creates minimal session code that does only one "nop" opcode
pub fn do_minimum_bytes() -> Vec<u8> {
    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
        .with_instructions(Instructions::new(vec![Instruction::Nop, Instruction::End]))
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
fn should_charge_minimum_for_do_nothing_session() {
    let minimum_deploy_payment = U512::from(0);

    let do_nothing_request = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash = [42; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_bytes(wasm_utils::do_nothing_bytes(), session_args)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => minimum_deploy_payment,
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();

    let proposer_balance_before = builder.get_proposer_purse_balance();

    let account_balance_before = builder.get_purse_balance(account.main_purse());

    builder.exec(do_nothing_request).commit();

    let error = builder.get_error().unwrap();
    assert!(
        matches!(error, CoreError::InsufficientPayment),
        "{:?}",
        error
    );

    let gas = builder.last_exec_gas_cost();
    assert_eq!(gas, Gas::zero());

    let account_balance_after = builder.get_purse_balance(account.main_purse());
    let proposer_balance_after = builder.get_proposer_purse_balance();

    assert_eq!(account_balance_before - *MAX_PAYMENT, account_balance_after);
    assert_eq!(
        proposer_balance_before + *MAX_PAYMENT,
        proposer_balance_after
    );
}

#[ignore]
#[test]
fn should_execute_do_minimum_session() {
    let minimum_deploy_payment = U512::from(DEFAULT_NOP_COST);

    let do_minimum_request = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash = [42; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_bytes(do_minimum_bytes(), session_args)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => minimum_deploy_payment,
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();

    let proposer_balance_before = builder.get_proposer_purse_balance();

    let account_balance_before = builder.get_purse_balance(account.main_purse());

    builder.exec(do_minimum_request).expect_success().commit();

    let gas = builder.last_exec_gas_cost();
    assert_eq!(gas, Gas::from(DEFAULT_NOP_COST));

    let account_balance_after = builder.get_purse_balance(account.main_purse());
    let proposer_balance_after = builder.get_proposer_purse_balance();

    assert_eq!(
        account_balance_before - minimum_deploy_payment,
        account_balance_after
    );
    assert_eq!(
        proposer_balance_before + minimum_deploy_payment,
        proposer_balance_after
    );
}

#[ignore]
#[test]
fn should_charge_minimum_for_do_nothing_payment() {
    let minimum_deploy_payment = U512::from(0);

    let do_nothing_request = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash = [42; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_bytes(wasm_utils::do_nothing_bytes(), session_args)
            .with_payment_bytes(
                wasm_utils::do_nothing_bytes(),
                runtime_args! {
                    ARG_AMOUNT => minimum_deploy_payment,
                },
            )
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();

    let proposer_balance_before = builder.get_proposer_purse_balance();

    let account_balance_before = builder.get_purse_balance(account.main_purse());

    builder.exec(do_nothing_request).commit();

    let error = builder.get_error().unwrap();
    assert!(
        matches!(error, CoreError::InsufficientPayment),
        "{:?}",
        error
    );

    let gas = builder.last_exec_gas_cost();
    assert_eq!(gas, Gas::zero());

    let account_balance_after = builder.get_purse_balance(account.main_purse());
    let proposer_balance_after = builder.get_proposer_purse_balance();

    assert_eq!(account_balance_before - *MAX_PAYMENT, account_balance_after);
    assert_eq!(
        proposer_balance_before + *MAX_PAYMENT,
        proposer_balance_after
    );
}
