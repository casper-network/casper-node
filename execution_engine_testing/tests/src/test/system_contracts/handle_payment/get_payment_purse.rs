use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT,
    MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{account::AccountHash, runtime_args, RuntimeArgs, U512};

const CONTRACT_GET_PAYMENT_PURSE: &str = "get_payment_purse.wasm";
const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const ACCOUNT_1_INITIAL_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";

#[ignore]
#[test]
fn should_run_get_payment_purse_contract_default_account() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GET_PAYMENT_PURSE,
        runtime_args! {
            ARG_AMOUNT => *DEFAULT_PAYMENT,
        },
    )
    .build();
    InMemoryWasmTestBuilder::default()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_run_get_payment_purse_contract_account_1() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
       *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => U512::from(ACCOUNT_1_INITIAL_BALANCE) },
    )
        .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_GET_PAYMENT_PURSE,
        runtime_args! {
            ARG_AMOUNT => *DEFAULT_PAYMENT,
        },
    )
    .build();
    InMemoryWasmTestBuilder::default()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2)
        .expect_success()
        .commit();
}
