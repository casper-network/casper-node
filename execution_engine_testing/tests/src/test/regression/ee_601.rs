use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, LOCAL_GENESIS_REQUEST,
};
use casper_types::{runtime_args, CLValue, EntityAddr, RuntimeArgs, StoredValue};

const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_run_ee_601_pay_session_new_uref_collision() {
    let genesis_account_hash = *DEFAULT_ACCOUNT_ADDR;

    let deploy_item = DeployItemBuilder::new()
        .with_deploy_hash([1; 32])
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_payment_code(
            "ee_601_regression.wasm",
            runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT },
        )
        .with_session_code("ee_601_regression.wasm", RuntimeArgs::default())
        .with_authorization_keys(&[genesis_account_hash])
        .build();

    let exec_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .expect_success()
        .commit();

    let hash = *DEFAULT_ACCOUNT_ADDR;
    let named_keys = builder.get_named_keys(EntityAddr::Account(hash.value()));

    let payment_uref = *named_keys
        .get("new_uref_result-payment")
        .expect("payment uref should exist");

    let session_uref = *named_keys
        .get("new_uref_result-session")
        .expect("session uref should exist");

    assert_ne!(
        payment_uref, session_uref,
        "payment and session code should not create same uref"
    );

    let payment_value: StoredValue = builder
        .query(None, payment_uref, &[])
        .expect("should find payment value");

    assert_eq!(
        payment_value,
        StoredValue::CLValue(CLValue::from_t("payment".to_string()).unwrap()),
        "expected payment"
    );

    let session_value: StoredValue = builder
        .query(None, session_uref, &[])
        .expect("should find session value");

    assert_eq!(
        session_value,
        StoredValue::CLValue(CLValue::from_t("session".to_string()).unwrap()),
        "expected session"
    );
}
