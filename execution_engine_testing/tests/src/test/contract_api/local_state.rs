use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{bytesrepr::ToBytes, CLType, Key, RuntimeArgs};

const LOCAL_KEY_NAME: &str = "local";
const LOCAL_KEY: [u8; 32] = [66u8; 32];
const LOCAL_STATE_WASM: &str = "local_state.wasm";

#[ignore]
#[test]
fn should_revert() {
    let local_state_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        LOCAL_STATE_WASM,
        RuntimeArgs::default(),
    )
    .build();

    let local_state_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        LOCAL_STATE_WASM,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder
        .exec(local_state_request_1)
        .commit()
        .expect_success();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let stored_local_key = account.named_keys().get(LOCAL_KEY_NAME).expect("local key");
    let local_key_uref = stored_local_key.into_uref().expect("should be uref");

    let key_bytes = LOCAL_KEY.to_bytes().unwrap();
    let local_key = Key::local(local_key_uref, &key_bytes);

    let stored_value = builder
        .query(None, local_key_uref.into(), &[])
        .expect("should have value");
    let local_uref_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should have cl value");
    assert_eq!(
        local_uref_value.cl_type(),
        &CLType::Unit,
        "created local key uref should be unit"
    );

    let stored_value = builder
        .query(None, local_key, &[])
        .expect("should have value");
    let local_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should have cl value");

    let s: String = local_value.into_t().expect("should be a string");
    assert_eq!(s, "Hello, world!");

    builder
        .exec(local_state_request_2)
        .commit()
        .expect_success();

    let stored_value = builder
        .query(None, local_key, &[])
        .expect("should have value");
    let local_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should have cl value");

    let s: String = local_value.into_t().expect("should be a string");
    assert_eq!(s, "Hello, world! Hello, world!");
}
