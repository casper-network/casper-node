use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{account::AccountHash, runtime_args, ApiError, RuntimeArgs, U512};

const FAUCET_CONTRACT: &str = "faucet.wasm";
const NEW_ACCOUNT_ADDR: AccountHash = AccountHash::new([99u8; 32]);

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_get_funds_from_faucet() {
    let amount = U512::from(1000);
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        FAUCET_CONTRACT,
        runtime_args! { ARG_TARGET => NEW_ACCOUNT_ADDR, ARG_AMOUNT => amount },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .expect_success()
        .commit();

    let account = builder
        .get_account(NEW_ACCOUNT_ADDR)
        .expect("should get account");

    let account_purse = account.main_purse();
    let account_balance = builder.get_purse_balance(account_purse);
    assert_eq!(
        account_balance, amount,
        "faucet should have created account with requested amount"
    );
}

#[ignore]
#[test]
fn should_fail_if_already_funded() {
    let amount = U512::from(1000);
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        FAUCET_CONTRACT,
        runtime_args! { ARG_TARGET => NEW_ACCOUNT_ADDR, ARG_AMOUNT => amount },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        FAUCET_CONTRACT,
        runtime_args! { ARG_TARGET => NEW_ACCOUNT_ADDR, ARG_AMOUNT => amount },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder
        .run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2); // should fail

    let error_msg = builder
        .exec_error_message(1)
        .expect("should have error message");
    assert!(
        error_msg.contains(&format!("{:?}", ApiError::User(1))),
        error_msg
    );
}
