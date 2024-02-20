use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, ARG_AMOUNT,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    engine_state::{self, Error},
    execution,
};
use casper_storage::{system::transfer::TransferError, tracking_copy::TrackingCopyError};
use casper_types::{
    account::AccountHash, addressable_entity::Weight, runtime_args, system::mint, U512,
};

const CONTRACT_ADD_ASSOCIATED_KEY: &str = "add_associated_key.wasm";
const CONTRACT_ADD_UPDATE_ASSOCIATED_KEY: &str = "add_update_associated_key.wasm";
const CONTRACT_SET_ACTION_THRESHOLDS: &str = "set_action_thresholds.wasm";
const ARG_KEY_MANAGEMENT_THRESHOLD: &str = "key_management_threshold";
const ARG_DEPLOY_THRESHOLD: &str = "deploy_threshold";
const ARG_ACCOUNT: &str = "account";
const ARG_WEIGHT: &str = "weight";
const KEY_1: AccountHash = AccountHash::new([254; 32]);
const KEY_2: AccountHash = AccountHash::new([253; 32]);
const KEY_2_WEIGHT: Weight = Weight::new(100);
const KEY_3: AccountHash = AccountHash::new([252; 32]);

#[ignore]
#[test]
fn should_deploy_with_authorized_identity_key() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_SET_ACTION_THRESHOLDS,
        runtime_args! {
            ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(1),
            ARG_DEPLOY_THRESHOLD => Weight::new(1),
        },
    )
    .build();
    // Basic deploy with single key
    LmdbWasmTestBuilder::default()
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit()
        .expect_success();
}

#[ignore]
#[test]
fn should_raise_auth_failure_with_invalid_key() {
    // tests that authorized keys that does not belong to account raises
    // Error::Authorization
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_1);

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_session_code(
                CONTRACT_SET_ACTION_THRESHOLDS,
                runtime_args! {
                    ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(1),
                    ARG_DEPLOY_THRESHOLD => Weight::new(1)
                },
            )
            .with_deploy_hash([1u8; 32])
            .with_authorization_keys(&[KEY_1])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    // Basic deploy with single key
    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit();

    let deploy_result = builder
        .get_exec_result_owned(0)
        .expect("should have exec response")
        .get(0)
        .cloned()
        .expect("should have at least one deploy result");

    assert!(
        deploy_result.has_precondition_failure(),
        "{:?}",
        deploy_result
    );
    let message = format!("{}", deploy_result.as_error().unwrap());

    assert_eq!(
        message,
        format!(
            "{}",
            engine_state::Error::TrackingCopy(TrackingCopyError::Authorization)
        )
    )
}

#[ignore]
#[test]
fn should_raise_auth_failure_with_invalid_keys() {
    // tests that authorized keys that does not belong to account raises
    // Error::Authorization
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_1);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_2);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_3);

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_session_code(
                CONTRACT_SET_ACTION_THRESHOLDS,
                runtime_args! {
                    ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(1),
                    ARG_DEPLOY_THRESHOLD => Weight::new(1)
                },
            )
            .with_deploy_hash([1u8; 32])
            .with_authorization_keys(&[KEY_2, KEY_1, KEY_3])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    // Basic deploy with single key
    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit();

    let deploy_result = builder
        .get_exec_result_owned(0)
        .expect("should have exec response")
        .get(0)
        .cloned()
        .expect("should have at least one deploy result");

    assert!(deploy_result.has_precondition_failure());
    let message = format!("{}", deploy_result.as_error().unwrap());

    assert_eq!(
        message,
        format!(
            "{}",
            engine_state::Error::TrackingCopy(TrackingCopyError::Authorization)
        )
    )
}

#[ignore]
#[test]
fn should_raise_deploy_authorization_failure() {
    // tests that authorized keys needs sufficient cumulative weight
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_1);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_2);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_3);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { ARG_ACCOUNT => KEY_1, },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { ARG_ACCOUNT => KEY_2, },
    )
    .build();
    let exec_request_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { ARG_ACCOUNT => KEY_3, },
    )
    .build();
    // Deploy threshold is equal to 3, keymgmnt is still 1.
    // Even after verifying weights and thresholds to not
    // lock out the account, those values should work as
    // account now has 1. identity key with weight=1 and
    // a key with weight=2.
    let exec_request_4 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_SET_ACTION_THRESHOLDS,
        runtime_args! {
            ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(4),
            ARG_DEPLOY_THRESHOLD => Weight::new(3)
        },
    )
    .build();
    // Basic deploy with single key
    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
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
        // This should execute successfully - change deploy and key management
        // thresholds.
        .exec(exec_request_4)
        .expect_success()
        .commit();

    let exec_request_5 = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            // Next deploy will see deploy threshold == 4, keymgmnt == 5
            .with_session_code(
                CONTRACT_SET_ACTION_THRESHOLDS,
                runtime_args! {
                    ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(5),
                    ARG_DEPLOY_THRESHOLD => Weight::new(4)
                }, //args
            )
            .with_deploy_hash([5u8; 32])
            .with_authorization_keys(&[KEY_1])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    // With deploy threshold == 3 using single secondary key
    // with weight == 2 should raise deploy authorization failure.
    builder.clear_results().exec(exec_request_5).commit();

    {
        let deploy_result = builder
            .get_exec_result_owned(0)
            .expect("should have exec response")
            .get(0)
            .cloned()
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
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            // change deployment threshold to 4
            .with_session_code(
                CONTRACT_SET_ACTION_THRESHOLDS,
                runtime_args! {
                    ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(6),
                    ARG_DEPLOY_THRESHOLD => Weight::new(5)
                },
            )
            .with_deploy_hash([6u8; 32])
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR, KEY_1, KEY_2, KEY_3])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };
    // identity key (w: 1) and KEY_1 (w: 2) passes threshold of 3
    builder
        .clear_results()
        .exec(exec_request_6)
        .expect_success()
        .commit();

    let exec_request_7 = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            // change deployment threshold to 4
            .with_session_code(
                CONTRACT_SET_ACTION_THRESHOLDS,
                runtime_args! {
                    ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(0),
                    ARG_DEPLOY_THRESHOLD => Weight::new(0)
                }, //args
            )
            .with_deploy_hash([6u8; 32])
            .with_authorization_keys(&[KEY_2, KEY_1])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    // deployment threshold is now 4
    // failure: KEY_2 weight + KEY_1 weight < deployment threshold
    // let result4 = builder.clear_results()
    builder.clear_results().exec(exec_request_7).commit();

    {
        let deploy_result = builder
            .get_exec_result_owned(0)
            .expect("should have exec response")
            .get(0)
            .cloned()
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
                CONTRACT_SET_ACTION_THRESHOLDS,
                runtime_args! {
                    ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(0),
                    ARG_DEPLOY_THRESHOLD => Weight::new(0)
                }, //args
            )
            .with_deploy_hash([8u8; 32])
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR, KEY_1, KEY_2, KEY_3])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    // success: identity key weight + KEY_1 weight + KEY_2 weight >= deployment
    // threshold
    builder
        .clear_results()
        .exec(exec_request_8)
        .commit()
        .expect_success();
}

#[ignore]
#[test]
fn should_authorize_deploy_with_multiple_keys() {
    // tests that authorized keys needs sufficient cumulative weight
    // and each of the associated keys is greater than threshold
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_1);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_2);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { ARG_ACCOUNT => KEY_1, },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { ARG_ACCOUNT => KEY_2, },
    )
    .build();
    // Basic deploy with single key
    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        // Reusing a test contract that would add new key
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2)
        .expect_success()
        .commit();

    // KEY_1 (w: 2) KEY_2 (w: 2) each passes default threshold of 1

    let exec_request_3 = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_session_code(
                CONTRACT_SET_ACTION_THRESHOLDS,
                runtime_args! {
                    ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(0),
                    ARG_DEPLOY_THRESHOLD => Weight::new(0),
                },
            )
            .with_deploy_hash([36; 32])
            .with_authorization_keys(&[KEY_2, KEY_1])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    builder.exec(exec_request_3).expect_success().commit();
}

#[ignore]
#[test]
fn should_not_authorize_deploy_with_duplicated_keys() {
    // tests that authorized keys needs sufficient cumulative weight
    // and each of the associated keys is greater than threshold

    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_1);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { ARG_ACCOUNT => KEY_1, },
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_ASSOCIATED_KEY,
        runtime_args! {
            ARG_ACCOUNT => KEY_2,
            ARG_WEIGHT => KEY_2_WEIGHT,
        },
    )
    .build();

    let exec_request_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_SET_ACTION_THRESHOLDS,
        runtime_args! {
            ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(4),
            ARG_DEPLOY_THRESHOLD => Weight::new(3)
        },
    )
    .build();
    // Basic deploy with single key
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder
        // Reusing a test contract that would add new key
        .exec(exec_request_1)
        .expect_success()
        .commit();

    builder.exec(exec_request_2).expect_success().commit();

    builder.exec(exec_request_3).expect_success().commit();

    let exec_request_3 = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT,
            })
            .with_session_code(
                CONTRACT_SET_ACTION_THRESHOLDS,
                runtime_args! {
                    ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(0),
                    ARG_DEPLOY_THRESHOLD => Weight::new(0)
                },
            )
            .with_deploy_hash([3u8; 32])
            .with_authorization_keys(&[
                KEY_1, KEY_1, KEY_1, KEY_1, KEY_1, KEY_1, KEY_1, KEY_1, KEY_1, KEY_1,
            ])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };
    builder.clear_results().exec(exec_request_3).commit();
    let deploy_result = builder
        .get_exec_result_owned(0)
        .expect("should have exec response")
        .get(0)
        .cloned()
        .expect("should have at least one deploy result");

    assert!(
        deploy_result.has_precondition_failure(),
        "{:?}",
        deploy_result
    );
    let message = format!("{}", deploy_result.as_error().unwrap());
    assert!(message.contains(&format!(
        "{}",
        TrackingCopyError::DeploymentAuthorizationFailure
    )))
}

#[ignore]
#[test]
fn should_not_authorize_transfer_without_deploy_key_threshold() {
    // tests that authorized keys needs sufficient cumulative weight
    // and each of the associated keys is greater than threshold
    let transfer_amount = U512::from(1);

    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_1);
    assert_ne!(*DEFAULT_ACCOUNT_ADDR, KEY_2);

    let add_key_1_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { ARG_ACCOUNT => KEY_1, },
    )
    .build();
    let add_key_2_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { ARG_ACCOUNT => KEY_2, },
    )
    .build();
    let update_thresholds_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_SET_ACTION_THRESHOLDS,
        runtime_args! {
            ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(5),
            ARG_DEPLOY_THRESHOLD => Weight::new(5),
        },
    )
    .build();

    // Basic deploy with single key
    let mut builder = LmdbWasmTestBuilder::default();

    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        // Reusing a test contract that would add new key
        .exec(add_key_1_request)
        .expect_success()
        .commit();

    builder.exec(add_key_2_request).expect_success().commit();

    builder
        .exec(update_thresholds_request)
        .expect_success()
        .commit();

    // KEY_1 (w: 2) DEFAULT_ACCOUNT (w: 1) does not pass deploy threshold of 5
    let id: Option<u64> = None;

    let transfer_request_1 = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_transfer_args(runtime_args! {
                mint::ARG_TARGET => KEY_2,
                mint::ARG_AMOUNT => transfer_amount,
                mint::ARG_ID => id,
            })
            .with_deploy_hash([36; 32])
            .with_authorization_keys(&[KEY_1, *DEFAULT_ACCOUNT_ADDR])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    builder.exec(transfer_request_1).commit();

    let response = builder
        .get_exec_result_owned(3)
        .expect("should have response")
        .first()
        .cloned()
        .expect("should have first result");
    let error = response.as_error().expect("should have error");
    assert!(matches!(
        error,
        Error::Transfer(TransferError::TrackingCopy(
            TrackingCopyError::DeploymentAuthorizationFailure
        ))
    ));

    // KEY_1 (w: 2) KEY_2 (w: 2) DEFAULT_ACCOUNT_ADDR (w: 1) each passes threshold of 5
    let id: Option<u64> = None;

    let transfer_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_transfer_args(runtime_args! {
                mint::ARG_TARGET => KEY_2,
                mint::ARG_AMOUNT => transfer_amount,
                mint::ARG_ID => id,
            })
            .with_deploy_hash([37; 32])
            .with_authorization_keys(&[KEY_2, KEY_1, *DEFAULT_ACCOUNT_ADDR])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    builder.exec(transfer_request).expect_success().commit();
}
