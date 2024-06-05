use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    LOCAL_GENESIS_REQUEST,
};
use casper_types::{runtime_args, Gas, RuntimeArgs, DEFAULT_NOP_COST, U512};

use crate::wasm_utils;

const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_charge_minimum_for_do_nothing_session() {
    let minimum_deploy_payment = U512::from(0);

    let account_hash = *DEFAULT_ACCOUNT_ADDR;
    let session_args = RuntimeArgs::default();
    let deploy_hash = [42; 32];

    let deploy_item = DeployItemBuilder::new()
        .with_address(account_hash)
        .with_session_bytes(wasm_utils::do_nothing_bytes(), session_args)
        .with_standard_payment(runtime_args! {
            ARG_AMOUNT => minimum_deploy_payment,
        })
        .with_authorization_keys(&[account_hash])
        .with_deploy_hash(deploy_hash)
        .build();

    let do_nothing_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    builder.exec(do_nothing_request).commit();

    let gas = builder.last_exec_gas_cost();
    assert_eq!(gas, Gas::zero());
}

#[ignore]
#[test]
fn should_execute_do_minimum_session() {
    let minimum_deploy_payment = U512::from(DEFAULT_NOP_COST);

    let account_hash = *DEFAULT_ACCOUNT_ADDR;
    let session_args = RuntimeArgs::default();
    let deploy_hash = [42; 32];

    let deploy_item = DeployItemBuilder::new()
        .with_address(account_hash)
        .with_session_bytes(wasm_utils::do_minimum_bytes(), session_args)
        .with_standard_payment(runtime_args! {
            ARG_AMOUNT => minimum_deploy_payment,
        })
        .with_authorization_keys(&[account_hash])
        .with_deploy_hash(deploy_hash)
        .build();

    let do_minimum_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    builder.exec(do_minimum_request).expect_success().commit();

    let gas = builder.last_exec_gas_cost();
    assert_eq!(gas, Gas::from(DEFAULT_NOP_COST));
}

#[ignore]
#[test]
fn should_charge_minimum_for_do_nothing_payment() {
    let minimum_deploy_payment = U512::from(0);

    let account_hash = *DEFAULT_ACCOUNT_ADDR;
    let session_args = RuntimeArgs::default();
    let deploy_hash = [42; 32];

    let deploy_item = DeployItemBuilder::new()
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

    let do_nothing_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    builder.exec(do_nothing_request).commit();

    let gas = builder.last_exec_gas_cost();
    assert_eq!(gas, Gas::zero());
}
