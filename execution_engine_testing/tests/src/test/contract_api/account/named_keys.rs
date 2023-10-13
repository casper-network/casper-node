use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{bytesrepr::FromBytes, runtime_args, CLTyped, Key, URef, U512};

const CONTRACT_NAMED_KEYS: &str = "named_keys.wasm";
const EXPECTED_UREF_VALUE: u64 = 123_456_789u64;

const KEY1: &str = "hello-world";
const KEY2: &str = "big-value";

const COMMAND_CREATE_UREF1: &str = "create-uref1";
const COMMAND_CREATE_UREF2: &str = "create-uref2";
const COMMAND_REMOVE_UREF1: &str = "remove-uref1";
const COMMAND_REMOVE_UREF2: &str = "remove-uref2";
const COMMAND_TEST_READ_UREF1: &str = "test-read-uref1";
const COMMAND_TEST_READ_UREF2: &str = "test-read-uref2";
const COMMAND_INCREASE_UREF2: &str = "increase-uref2";
const COMMAND_OVERWRITE_UREF2: &str = "overwrite-uref2";
const ARG_COMMAND: &str = "command";

fn run_command(builder: &mut LmdbWasmTestBuilder, command: &str) {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAMED_KEYS,
        runtime_args! { ARG_COMMAND => command },
    )
    .build();
    builder.exec(exec_request).commit().expect_success();
}

fn read_value<T: CLTyped + FromBytes>(builder: &mut LmdbWasmTestBuilder, key: URef) -> T {
    builder
        .query_uref_value(None, Key::URef(key), &[])
        .expect("should have value")
        .into_t()
        .expect("should convert successfully")
}

#[ignore]
#[test]
fn should_run_named_keys_contract() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    run_command(&mut builder, COMMAND_CREATE_UREF1);

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    assert!(account.named_keys().contains(KEY1));
    assert!(!account.named_keys().contains(KEY2));

    run_command(&mut builder, COMMAND_CREATE_UREF2);

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let uref1 = account
        .named_keys()
        .get(KEY1)
        .expect("should have key")
        .into_uref()
        .expect("should be uref");
    let uref2 = account
        .named_keys()
        .get(KEY2)
        .expect("should have key")
        .into_uref()
        .expect("should be uref");
    let value1: String = read_value(&mut builder, uref1);
    let value2: U512 = read_value(&mut builder, uref2);
    assert_eq!(value1, "Hello, world!");
    assert_eq!(value2, U512::max_value());

    run_command(&mut builder, COMMAND_TEST_READ_UREF1);

    run_command(&mut builder, COMMAND_REMOVE_UREF1);

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    assert!(!account.named_keys().contains(KEY1));
    assert!(account.named_keys().contains(KEY2));

    run_command(&mut builder, COMMAND_TEST_READ_UREF2);

    run_command(&mut builder, COMMAND_INCREASE_UREF2);

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let uref2 = account
        .named_keys()
        .get(KEY2)
        .expect("should have key")
        .into_uref()
        .expect("should be uref");
    let value2: U512 = read_value(&mut builder, uref2);
    assert_eq!(value2, U512::zero());

    run_command(&mut builder, COMMAND_OVERWRITE_UREF2);

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let uref2 = account
        .named_keys()
        .get(KEY2)
        .expect("should have key")
        .into_uref()
        .expect("should be uref");
    let value2: U512 = read_value(&mut builder, uref2);
    assert_eq!(value2, U512::from(EXPECTED_UREF_VALUE));

    run_command(&mut builder, COMMAND_REMOVE_UREF2);

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    assert!(!account.named_keys().contains(KEY1));
    assert!(!account.named_keys().contains(KEY2));
}
