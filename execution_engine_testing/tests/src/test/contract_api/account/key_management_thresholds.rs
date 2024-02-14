use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, ARG_AMOUNT,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{account::AccountHash, runtime_args};

const CONTRACT_KEY_MANAGEMENT_THRESHOLDS: &str = "key_management_thresholds.wasm";

const ARG_STAGE: &str = "stage";

#[ignore]
#[test]
fn should_verify_key_management_permission_with_low_weight() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_KEY_MANAGEMENT_THRESHOLDS,
        runtime_args! { ARG_STAGE => String::from("init") },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_KEY_MANAGEMENT_THRESHOLDS,
        runtime_args! { ARG_STAGE => String::from("test-permission-denied") },
    )
    .build();
    LmdbWasmTestBuilder::default()
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2)
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_verify_key_management_permission_with_sufficient_weight() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_KEY_MANAGEMENT_THRESHOLDS,
        runtime_args! { ARG_STAGE => String::from("init") },
    )
    .build();
    let exec_request_2 = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            // This test verifies that all key management operations succeed
            .with_session_code(
                "key_management_thresholds.wasm",
                runtime_args! { ARG_STAGE => String::from("test-key-mgmnt-succeed") },
            )
            .with_deploy_hash([2u8; 32])
            .with_authorization_keys(&[
                *DEFAULT_ACCOUNT_ADDR,
                // Key [42; 32] is created in init stage
                AccountHash::new([42; 32]),
            ])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };
    LmdbWasmTestBuilder::default()
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2)
        .expect_success()
        .commit();
}
