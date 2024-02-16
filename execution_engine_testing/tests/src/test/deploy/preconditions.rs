use assert_matches::assert_matches;

use casper_engine_test_support::{
    utils, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::Error;
use casper_types::{account::AccountHash, runtime_args, RuntimeArgs, U512};

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([42u8; 32]);
const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_raise_precondition_authorization_failure_invalid_account() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let nonexistent_account_addr = AccountHash::new([99u8; 32]);
    let payment_purse_amount = 10_000_000;
    let transferred_amount = 1;

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_session_code(
                "transfer_purse_to_account.wasm",
                runtime_args! { "target" =>account_1_account_hash, "amount" => U512::from(transferred_amount) },
            )
            // .with_address(nonexistent_account_addr)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => U512::from(payment_purse_amount) })
            .with_authorization_keys(&[nonexistent_account_addr])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit();

    let response = builder
        .get_exec_result_owned(0)
        .expect("there should be a response");

    let precondition_failure = utils::get_precondition_failure(&response);
    assert_matches!(precondition_failure, Error::Authorization);
}

#[ignore]
#[test]
fn should_raise_precondition_authorization_failure_empty_authorized_keys() {
    let empty_keys: [AccountHash; 0] = [];
    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code("do_nothing.wasm", RuntimeArgs::default())
            .with_empty_payment_bytes(RuntimeArgs::default())
            .with_deploy_hash([1; 32])
            // empty authorization keys to force error
            .with_authorization_keys(&empty_keys)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit();

    let response = builder
        .get_exec_result_owned(0)
        .expect("there should be a response");

    let precondition_failure = utils::get_precondition_failure(&response);
    assert_matches!(precondition_failure, Error::Authorization);
}

#[ignore]
#[test]
fn should_raise_precondition_authorization_failure_invalid_authorized_keys() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let nonexistent_account_addr = AccountHash::new([99u8; 32]);
    let payment_purse_amount = 10_000_000;
    let transferred_amount = 1;

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_session_code(
                "transfer_purse_to_account.wasm",
                runtime_args! { "target" =>account_1_account_hash, "amount" => U512::from(transferred_amount) },
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => U512::from(payment_purse_amount) })
            // invalid authorization key to force error
            .with_authorization_keys(&[nonexistent_account_addr])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit();

    let response = builder
        .get_exec_result_owned(0)
        .expect("there should be a response");

    let precondition_failure = utils::get_precondition_failure(&response);
    assert_matches!(precondition_failure, Error::Authorization);
}
