use std::collections::HashMap;

use assert_matches::assert_matches;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_KEY, DEFAULT_PAYMENT, LOCAL_GENESIS_REQUEST, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{engine_state::Error, execution::ExecError};
use casper_types::{
    account::AccountHash, execution::TransformKindV2, runtime_args, system::handle_payment,
    ApiError, Key, RuntimeArgs, U512,
};

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([42u8; 32]);
const DO_NOTHING_WASM: &str = "do_nothing.wasm";
const TRANSFER_PURSE_TO_ACCOUNT_WASM: &str = "transfer_purse_to_account.wasm";
const REVERT_WASM: &str = "revert.wasm";
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";

#[ignore]
#[test]
fn should_forward_payment_execution_runtime_error() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let transferred_amount = U512::from(1);

    let deploy_item = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_payment_code(REVERT_WASM, RuntimeArgs::default())
            .with_session_code(
                TRANSFER_PURSE_TO_ACCOUNT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => transferred_amount },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .build();

    let exec_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    builder.exec(exec_request).commit();

    let exec_result = builder
        .get_exec_result_owned(0)
        .expect("there should be a response");

    let error = exec_result.error().expect("should have error");
    assert_matches!(error, Error::Exec(ExecError::Revert(ApiError::User(100))));
}

#[ignore]
#[test]
fn independent_standard_payments_should_not_write_the_same_keys() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let payment_purse_amount = *DEFAULT_PAYMENT;
    let transfer_amount = MINIMUM_ACCOUNT_CREATION_BALANCE;

    let mut builder = LmdbWasmTestBuilder::default();

    let deploy_item = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(
                TRANSFER_PURSE_TO_ACCOUNT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => U512::from(transfer_amount) },
            )
            .with_standard_payment(runtime_args! { ARG_AMOUNT => payment_purse_amount })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([1; 32])
            .build();

    let setup_exec_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    // create another account via transfer
    builder
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(setup_exec_request)
        .expect_success()
        .commit();

    let deploy_item = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_session_code(DO_NOTHING_WASM, RuntimeArgs::default())
        .with_standard_payment(runtime_args! { ARG_AMOUNT => payment_purse_amount })
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
        .with_deploy_hash([2; 32])
        .build();

    let exec_request_from_genesis = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    let deploy_item = DeployItemBuilder::new()
        .with_address(ACCOUNT_1_ADDR)
        .with_session_code(DO_NOTHING_WASM, RuntimeArgs::default())
        .with_standard_payment(runtime_args! { ARG_AMOUNT => payment_purse_amount })
        .with_authorization_keys(&[account_1_account_hash])
        .with_deploy_hash([1; 32])
        .build();

    let exec_request_from_account_1 = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    // run two independent deploys
    builder
        .exec(exec_request_from_genesis)
        .expect_success()
        .commit()
        .exec(exec_request_from_account_1)
        .expect_success()
        .commit();

    let effects = builder.get_effects();
    let effects_from_genesis = &effects[1];
    let effects_from_account_1 = &effects[2];

    // Retrieve the payment purse.
    let payment_purse = builder
        .get_handle_payment_contract()
        .named_keys()
        .get(handle_payment::PAYMENT_PURSE_KEY)
        .unwrap()
        .into_uref()
        .unwrap();

    let transforms_from_genesis_map: HashMap<Key, TransformKindV2> = effects_from_genesis
        .transforms()
        .iter()
        .map(|transform| (*transform.key(), transform.kind().clone()))
        .collect();
    let transforms_from_account_1_map: HashMap<Key, TransformKindV2> = effects_from_account_1
        .transforms()
        .iter()
        .map(|transform| (*transform.key(), transform.kind().clone()))
        .collect();

    // Confirm the two deploys have no overlapping writes except for the payment purse balance.
    let common_write_keys = effects_from_genesis
        .transforms()
        .iter()
        .filter_map(|transform| {
            if transform.key() != &Key::Balance(payment_purse.addr())
                && matches!(
                    (
                        transforms_from_genesis_map.get(transform.key()),
                        transforms_from_account_1_map.get(transform.key()),
                    ),
                    (
                        Some(TransformKindV2::Write(_)),
                        Some(TransformKindV2::Write(_))
                    )
                )
            {
                Some(*transform.key())
            } else {
                None
            }
        });

    assert_eq!(common_write_keys.count(), 0);
}
