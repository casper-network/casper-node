use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{account::AccountHash, addressable_entity::Weight, runtime_args, U512};

const CONTRACT_ADD_UPDATE_ASSOCIATED_KEY: &str = "add_update_associated_key.wasm";
const CONTRACT_REMOVE_ASSOCIATED_KEY: &str = "remove_associated_key.wasm";
const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const ARG_ACCOUNT: &str = "account";

static ACCOUNT_1_INITIAL_FUND: Lazy<U512> = Lazy::new(|| *DEFAULT_PAYMENT * 10);

#[ignore]
#[test]
fn should_manage_associated_key() {
    // for a given account, should be able to add a new associated key and update
    // that key
    let mut builder = LmdbWasmTestBuilder::default();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => ACCOUNT_1_ADDR, "amount" => *ACCOUNT_1_INITIAL_FUND },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_ADD_UPDATE_ASSOCIATED_KEY,
        runtime_args! { "account" => *DEFAULT_ACCOUNT_ADDR, },
    )
    .build();

    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .commit();

    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_2).expect_success().commit();

    let genesis_key = *DEFAULT_ACCOUNT_ADDR;

    let contract_1 = builder
        .get_entity_by_account_hash(ACCOUNT_1_ADDR)
        .expect("should have account");

    let gen_weight = contract_1
        .associated_keys()
        .get(&genesis_key)
        .expect("weight");

    let expected_weight = Weight::new(2);
    assert_eq!(*gen_weight, expected_weight, "unexpected weight");

    let exec_request_3 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_REMOVE_ASSOCIATED_KEY,
        runtime_args! { ARG_ACCOUNT => *DEFAULT_ACCOUNT_ADDR, },
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();

    let contract_1 = builder
        .get_entity_by_account_hash(ACCOUNT_1_ADDR)
        .expect("should have account");

    let new_weight = contract_1.associated_keys().get(&genesis_key);

    assert_eq!(new_weight, None, "key should be removed");

    let is_error = builder.is_error();
    assert!(!is_error);
}
