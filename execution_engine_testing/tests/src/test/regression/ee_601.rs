use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    execution::TransformKind, package::PackageKindTag, runtime_args, CLValue, Key, RuntimeArgs,
    StoredValue,
};

const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_run_ee_601_pay_session_new_uref_collision() {
    let genesis_account_hash = *DEFAULT_ACCOUNT_ADDR;

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_deploy_hash([1; 32])
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_payment_code(
                "ee_601_regression.wasm",
                runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT },
            )
            .with_session_code("ee_601_regression.wasm", RuntimeArgs::default())
            .with_authorization_keys(&[genesis_account_hash])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = LmdbWasmTestBuilder::default();

    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request);

    let entity_hash = builder
        .get_entity_hash_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract hash associated with default account");

    let entity_key = Key::addressable_entity_key(PackageKindTag::Account, entity_hash);

    let effects = &builder.get_effects()[0];
    let mut add_keys_iter = effects
        .transforms()
        .iter()
        .filter(|transform| transform.key() == &entity_key)
        .map(|transform| transform.kind());
    let payment_uref = match add_keys_iter.next().unwrap() {
        TransformKind::AddKeys(named_keys) => named_keys.get("new_uref_result-payment").unwrap(),
        _ => panic!("should be an AddKeys transform"),
    };
    let session_uref = match add_keys_iter.next().unwrap() {
        TransformKind::AddKeys(named_keys) => named_keys.get("new_uref_result-session").unwrap(),
        _ => panic!("should be an AddKeys transform"),
    };
    assert_ne!(
        payment_uref, session_uref,
        "payment and session code should not create same uref"
    );

    builder.commit();

    let payment_value: StoredValue = builder
        .query(None, *payment_uref, &[])
        .expect("should find payment value");

    assert_eq!(
        payment_value,
        StoredValue::CLValue(CLValue::from_t("payment".to_string()).unwrap()),
        "expected payment"
    );

    let session_value: StoredValue = builder
        .query(None, *session_uref, &[])
        .expect("should find session value");

    assert_eq!(
        session_value,
        StoredValue::CLValue(CLValue::from_t("session".to_string()).unwrap()),
        "expected session"
    );
}
