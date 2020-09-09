use casper_engine_test_support::{
    internal::{
        DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, ARG_AMOUNT,
        DEFAULT_PAYMENT, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::core::{engine_state, execution};
use casper_types::{
    account::{AccountHash, Weight},
    runtime_args, RuntimeArgs,
};

const CONTRACT_ADD_UPDATE_ASSOCIATED_KEY: &str = "add_update_associated_key.wasm";
const CONTRACT_AUTHORIZED_KEYS: &str = "authorized_keys.wasm";

#[ignore]
#[test]
fn should_deploy_with_authorized_identity_key() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUTHORIZED_KEYS,
        runtime_args! {
            "key_management_threshold" => Weight::new(1),
            "deploy_threshold" => Weight::new(1),
        },
    )
    .build();
    // Basic deploy with single key
    InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .expect_success();
}

#[ignore]
#[test]
fn should_raise_auth_failure_with_invalid_key() {
    // tests that authorized keys that does not belong to account raises
    // Error::Authorization
    let key_1 = AccountHash::new([254; 32]);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_1);

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(
                runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, },
            )
            .with_session_code(CONTRACT_AUTHORIZED_KEYS, runtime_args! { "key_management_threshold" => Weight::new(1), "deploy_threshold" => Weight::new(1) })
            .with_deploy_hash([1u8; 32])
            .with_authorization_keys(&[key_1])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    // Basic deploy with single key
    let result = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .finish();

    let deploy_result = result
        .builder()
        .get_exec_response(0)
        .expect("should have exec response")
        .get(0)
        .expect("should have at least one deploy result");

    assert!(
        deploy_result.has_precondition_failure(),
        "{:?}",
        deploy_result
    );
    let message = format!("{}", deploy_result.as_error().unwrap());

    assert_eq!(message, format!("{}", engine_state::Error::Authorization))
}

#[ignore]
#[test]
fn should_raise_auth_failure_with_invalid_keys() {
    // tests that authorized keys that does not belong to account raises
    // Error::Authorization
    let key_1 = AccountHash::new([254; 32]);
    let key_2 = AccountHash::new([253; 32]);
    let key_3 = AccountHash::new([252; 32]);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_1);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_2);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_3);

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(
                runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, },
            )
            .with_session_code("authorized_keys.wasm", runtime_args! { "key_management_threshold" => Weight::new(1), "deploy_threshold" => Weight::new(1) })
            .with_deploy_hash([1u8; 32])
            .with_authorization_keys(&[key_2, key_1, key_3])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    // Basic deploy with single key
    let result = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .finish();

    let deploy_result = result
        .builder()
        .get_exec_response(0)
        .expect("should have exec response")
        .get(0)
        .expect("should have at least one deploy result");

    assert!(deploy_result.has_precondition_failure());
    let message = format!("{}", deploy_result.as_error().unwrap());

    assert_eq!(message, format!("{}", engine_state::Error::Authorization))
}

#[ignore]
#[test]
fn should_raise_deploy_authorization_failure() {
    // tests that authorized keys needs sufficient cumulative weight
    let key_1 = AccountHash::new([254; 32]);
    let key_2 = AccountHash::new([253; 32]);
    let key_3 = AccountHash::new([252; 32]);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_1);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_2);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_3);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { "account" => key_1, },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { "account" => key_2, },
    )
    .build();
    let exec_request_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { "account" => key_3, },
    )
    .build();
    // Deploy threshold is equal to 3, keymgmnt is still 1.
    // Even after verifying weights and thresholds to not
    // lock out the account, those values should work as
    // account now has 1. identity key with weight=1 and
    // a key with weight=2.
    let exec_request_4 = ExecuteRequestBuilder::standard(
       *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUTHORIZED_KEYS,
        runtime_args! { "key_management_threshold" => Weight::new(4), "deploy_threshold" => Weight::new(3) },
    )
        .build();
    // Basic deploy with single key
    let result1 = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        // Reusing a test contract that would add new key
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2)
        .expect_success()
        .commit()
        .exec(exec_request_3)
        .expect_success()
        .commit()
        // This should execute successfuly - change deploy and key management
        // thresholds.
        .exec(exec_request_4)
        .expect_success()
        .commit()
        .finish();

    let exec_request_5 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUTHORIZED_KEYS,
        runtime_args! { "key_management_threshold" => Weight::new(5), "deploy_threshold" => Weight::new(4) }, //args
    )
    .build();

    // With deploy threshold == 3 using single secondary key
    // with weight == 2 should raise deploy authorization failure.
    let result2 = InMemoryWasmTestBuilder::from_result(result1)
        .exec(exec_request_5)
        .commit()
        .finish();

    {
        let deploy_result = result2
            .builder()
            .get_exec_response(0)
            .expect("should have exec response")
            .get(0)
            .expect("should have at least one deploy result");

        assert!(deploy_result.has_precondition_failure());
        let message = format!("{}", deploy_result.as_error().unwrap());
        assert!(message.contains(&format!(
            "{}",
            execution::Error::DeploymentAuthorizationFailure
        )))
    }
    let exec_request_6 = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(
                runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, },
            )
            // change deployment threshold to 4
            .with_session_code("authorized_keys.wasm", runtime_args! { "key_management_threshold" => Weight::new(6), "deploy_threshold" => Weight::new(5) })
            .with_deploy_hash([6u8; 32])
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR, key_1, key_2, key_3])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };
    // identity key (w: 1) and key_1 (w: 2) passes threshold of 3
    let result3 = InMemoryWasmTestBuilder::from_result(result2)
        .exec(exec_request_6)
        .expect_success()
        .commit()
        .finish();

    let exec_request_7 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUTHORIZED_KEYS,
        runtime_args! { "key_management_threshold" => Weight::new(0), "deploy_threshold" => Weight::new(0) }, //args
    )
    .build();

    // deployment threshold is now 4
    // failure: key_2 weight + key_1 weight < deployment threshold
    let result4 = InMemoryWasmTestBuilder::from_result(result3)
        .exec(exec_request_7)
        .commit()
        .finish();

    {
        let deploy_result = result4
            .builder()
            .get_exec_response(0)
            .expect("should have exec response")
            .get(0)
            .expect("should have at least one deploy result");

        assert!(deploy_result.has_precondition_failure());
        let message = format!("{}", deploy_result.as_error().unwrap());
        assert!(message.contains(&format!(
            "{}",
            execution::Error::DeploymentAuthorizationFailure
        )))
    }

    let exec_request_8 = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            // change deployment threshold to 4
            .with_session_code(
                "authorized_keys.wasm",
                runtime_args! { "key_management_threshold" => Weight::new(0), "deploy_threshold" => Weight::new(0) }, //args
            )
            .with_deploy_hash([8u8; 32])
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR, key_1, key_2, key_3])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    // success: identity key weight + key_1 weight + key_2 weight >= deployment
    // threshold
    InMemoryWasmTestBuilder::from_result(result4)
        .exec(exec_request_8)
        .commit()
        .expect_success()
        .finish();
}

#[ignore]
#[test]
fn should_authorize_deploy_with_multiple_keys() {
    // tests that authorized keys needs sufficient cumulative weight
    // and each of the associated keys is greater than threshold

    let key_1 = AccountHash::new([254; 32]);
    let key_2 = AccountHash::new([253; 32]);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_1);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_2);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { "account" => key_1, },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { "account" => key_2, },
    )
    .build();
    // Basic deploy with single key
    let result1 = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        // Reusing a test contract that would add new key
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2)
        .expect_success()
        .commit()
        .finish();

    // key_1 (w: 2) key_2 (w: 2) each passes default threshold of 1

    let exec_request_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUTHORIZED_KEYS,
        runtime_args! {
            "key_management_threshold" => Weight::new(0),
            "deploy_threshold" => Weight::new(0),
        },
    )
    .build();
    InMemoryWasmTestBuilder::from_result(result1)
        .exec(exec_request_3)
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_not_authorize_deploy_with_duplicated_keys() {
    // tests that authorized keys needs sufficient cumulative weight
    // and each of the associated keys is greater than threshold
    let key_1 = AccountHash::new([254; 32]);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, key_1);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { "account" => key_1, },
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
       *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUTHORIZED_KEYS,
        runtime_args! { "key_management_threshold" => Weight::new(4), "deploy_threshold" => Weight::new(3) },
    )
        .build();
    // Basic deploy with single key
    let result1 = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        // Reusing a test contract that would add new key
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2)
        .expect_success()
        .commit()
        .finish();

    let exec_request_3 = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(
                runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, },
            )
            .with_session_code("authorized_keys.wasm", runtime_args! { "key_management_threshold" => Weight::new(0), "deploy_threshold" => Weight::new(0) })
            .with_deploy_hash([3u8; 32])
            .with_authorization_keys(&[
                key_1, key_1, key_1, key_1, key_1, key_1, key_1, key_1, key_1, key_1,
            ])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };
    // success: identity key weight + key_1 weight + key_2 weight >= deployment
    // threshold
    let final_result = InMemoryWasmTestBuilder::from_result(result1)
        .exec(exec_request_3)
        .commit()
        .finish();
    let deploy_result = final_result
        .builder()
        .get_exec_response(0)
        .expect("should have exec response")
        .get(0)
        .expect("should have at least one deploy result");

    assert!(
        deploy_result.has_precondition_failure(),
        "{:?}",
        deploy_result
    );
    let message = format!("{}", deploy_result.as_error().unwrap());
    assert!(message.contains(&format!(
        "{}",
        execution::Error::DeploymentAuthorizationFailure
    )))
}
