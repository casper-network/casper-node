use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    addressable_entity::NamedKeyAddr, execution::TransformKind, runtime_args, CLValue, EntityAddr,
    Key, RuntimeArgs, StoredValue,
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
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request);

    let entity_hash = builder
        .get_entity_hash_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract hash associated with default account");

    let effects = &builder.get_effects()[0];

    let payment_uref_addr = NamedKeyAddr::new_from_string(
        EntityAddr::Account(entity_hash.value()),
        "new_uref_result-payment".to_string(),
    )
    .expect("must get addr");
    let payment_uref_key = Key::NamedKey(payment_uref_addr);

    let mut payment_transforms = effects
        .transforms()
        .iter()
        .filter(|transform| transform.key() == &payment_uref_key)
        .map(|transform| transform.kind());

    let payment_uref = match payment_transforms.next().unwrap() {
        TransformKind::Write(StoredValue::NamedKey(named_key)) => {
            named_key.get_key().expect("must get key")
        }
        _ => panic!("Should be Write transform"),
    };

    let session_uref_addr = NamedKeyAddr::new_from_string(
        EntityAddr::Account(entity_hash.value()),
        "new_uref_result-session".to_string(),
    )
    .expect("must get addr");

    let session_uref_key = Key::NamedKey(session_uref_addr);

    let mut session_transforms = effects
        .transforms()
        .iter()
        .filter(|transform| transform.key() == &session_uref_key)
        .map(|transform| transform.kind());

    let session_uref = match session_transforms.next().unwrap() {
        TransformKind::Write(StoredValue::NamedKey(named_key)) => {
            named_key.get_key().expect("must get key")
        }
        _ => panic!("Should be Write transform"),
    };

    builder.commit();

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
