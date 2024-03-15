use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, TransferRequestBuilder, DEFAULT_ACCOUNT_ADDR,
    LOCAL_GENESIS_REQUEST, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{engine_state, execution::ExecError};
use casper_types::{account::AccountHash, runtime_args, U512};

const ALICE_ADDR: AccountHash = AccountHash::new([42; 32]);

#[ignore]
#[test]
fn regression_20220222_escalate() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let transfer_request =
        TransferRequestBuilder::new(MINIMUM_ACCOUNT_CREATION_BALANCE, ALICE_ADDR).build();

    builder
        .transfer_and_commit(transfer_request)
        .expect_success();

    let alice = builder
        .get_entity_by_account_hash(ALICE_ADDR)
        .expect("should have account");

    let alice_main_purse = alice.main_purse();

    // Getting main purse URef to verify transfer
    let _source_purse = builder
        .get_expected_addressable_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
            engine_state::Error::Exec(ExecError::ForgedReference(forged_uref))
            if forged_uref == alice_main_purse.into_add()
        ),
        "Expected revert but received {:?}",
        error
    );
}
