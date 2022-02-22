use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::{engine_state, execution};
use casper_types::{account::AccountHash, runtime_args, RuntimeArgs, U512};

const ALICE_ADDR: AccountHash = AccountHash::new([42; 32]);

#[ignore]
#[test]
fn regression_20220222_escalate() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let transfer_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            "target" => ALICE_ADDR,
            "amount" => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            "id" => <Option<u64>>::None,
        },
    )
    .build();

    builder.exec(transfer_request).commit().expect_success();

    let alice = builder
        .get_account(ALICE_ADDR)
        .expect("should have account");

    let alice_main_purse = alice.main_purse();

    // Getting main purse URef to verify transfer
    let _source_purse = builder
        .get_expected_account(*DEFAULT_ACCOUNT_ADDR)
        .main_purse();

    let session_args = runtime_args! {
        "alice_purse_addr" => alice_main_purse.addr(),
        "amount" => U512::MAX,
    };

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "regression_20220222.wasm",
        session_args,
    )
    .build();
    builder.exec(exec_request).expect_failure();

    let error = builder.get_error().expect("should have error");

    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == alice_main_purse.into_add()
        ),
        "Expected revert but received {:?}",
        error
    );
}
