use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{account::AccountHash, runtime_args, RuntimeArgs, U512};

const CONTRACT_CREATE_PURSE_01: &str = "create_purse_01.wasm";
const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const TEST_PURSE_NAME: &str = "test_purse";
const ARG_PURSE_NAME: &str = "purse_name";

static ACCOUNT_1_INITIAL_BALANCE: Lazy<U512> = Lazy::new(|| *DEFAULT_PAYMENT);

#[ignore]
#[test]
fn should_insert_account_into_named_keys() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => ACCOUNT_1_ADDR, "amount" => *ACCOUNT_1_INITIAL_BALANCE},
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_CREATE_PURSE_01,
        runtime_args! { ARG_PURSE_NAME => TEST_PURSE_NAME },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_2).expect_success().commit();

    let account_1 = builder
        .get_account(ACCOUNT_1_ADDR)
        .expect("should have account");

    assert!(
        account_1.named_keys().contains_key(TEST_PURSE_NAME),
        "account_1 named_keys should include test purse"
    );
}

#[ignore]
#[test]
fn should_create_usable_purse() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => ACCOUNT_1_ADDR, "amount" => *ACCOUNT_1_INITIAL_BALANCE},
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_CREATE_PURSE_01,
        runtime_args! { ARG_PURSE_NAME => TEST_PURSE_NAME },
    )
    .build();
    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2)
        .expect_success()
        .commit();

    let account_1 = builder
        .get_account(ACCOUNT_1_ADDR)
        .expect("should have account");

    let purse = account_1
        .named_keys()
        .get(TEST_PURSE_NAME)
        .expect("should have known key")
        .into_uref()
        .expect("should have uref");

    let purse_balance = builder.get_purse_balance(purse);
    assert!(
        purse_balance.is_zero(),
        "when created directly a purse has 0 balance"
    );
}
