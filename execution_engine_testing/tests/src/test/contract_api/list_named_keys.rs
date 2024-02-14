use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{account::AccountHash, addressable_entity::NamedKeys, runtime_args, Key};

const CONTRACT_LIST_NAMED_KEYS: &str = "list_named_keys.wasm";
const NEW_NAME_ACCOUNT: &str = "Account";
const NEW_NAME_HASH: &str = "Hash";
const ARG_INITIAL_NAMED_KEYS: &str = "initial_named_args";
const ARG_NEW_NAMED_KEYS: &str = "new_named_keys";

#[ignore]
#[test]
fn should_list_named_keys() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let initial_named_keys: NamedKeys = NamedKeys::new();

    let new_named_keys = {
        let account_hash = AccountHash::new([1; 32]);
        let mut named_keys = NamedKeys::new();
        assert!(named_keys
            .insert(NEW_NAME_ACCOUNT.to_string(), Key::Account(account_hash))
            .is_none());
        assert!(named_keys
            .insert(NEW_NAME_HASH.to_string(), Key::Hash([2; 32]))
            .is_none());
        named_keys
    };

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_LIST_NAMED_KEYS,
        runtime_args! {
            ARG_INITIAL_NAMED_KEYS => initial_named_keys,
            ARG_NEW_NAMED_KEYS => new_named_keys,
        },
    )
    .build();

    builder.exec(exec_request).commit().expect_success();
}
