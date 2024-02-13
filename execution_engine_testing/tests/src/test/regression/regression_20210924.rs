use num_traits::Zero;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::{Error as CoreError, MAX_PAYMENT};
use casper_types::{runtime_args, Gas, RuntimeArgs, DEFAULT_NOP_COST, U512};

use crate::wasm_utils;

const ARG_AMOUNT: &str = "amount";

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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .unwrap();

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
            .with_session_bytes(wasm_utils::do_minimum_bytes(), session_args)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => minimum_deploy_payment,
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .unwrap();

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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .unwrap();

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
