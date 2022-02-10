use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{engine_state, execution};
use casper_types::{runtime_args, system::mint, AccessRights, ApiError, RuntimeArgs};

const REGRESSION_20220204_CONTRACT: &str = "regression_20220204.wasm";
const REGRESSION_20220204_CALL_CONTRACT: &str = "regression_20220204_call.wasm";
const TRANSFER_MAIN_PURSE_AS_SESSION: &str = "transfer_main_purse_as_session";
const ARG_ENTRYPOINT: &str = "entrypoint";
const ARG_PURSE: &str = "purse";
const TRANSFER_AS_CONTRACT: &str = "transfer_as_contract";
const TRANSFER_AS_SESSION: &str = "transfer_as_session";
const CONTRACT_HASH_NAME: &str = "regression-contract-hash";

#[ignore]
#[test]
fn regression_20220204_as_contract() {
    let mut builder = setup();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220204_CALL_CONTRACT,
        runtime_args! {
            ARG_ENTRYPOINT => TRANSFER_AS_CONTRACT,
        },
    )
    .build();

    builder.exec(exec_request_2).commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account
        .main_purse()
        .with_access_rights(AccessRights::READ_ADD_WRITE);

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == main_purse
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_as_contract_by_hash() {
    let mut builder = setup();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse().with_access_rights(AccessRights::READ);

    let exec_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        TRANSFER_AS_CONTRACT,
        runtime_args! {
            ARG_PURSE => main_purse,
        },
    )
    .build();

    builder.exec(exec_request).commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == main_purse.into_read_add_write()
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_as_session() {
    let mut builder = setup();

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220204_CALL_CONTRACT,
        runtime_args! {
            ARG_ENTRYPOINT => TRANSFER_AS_SESSION,
        },
    )
    .build();

    builder.exec(exec_request).commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse_read_only = account
        .main_purse()
        .with_access_rights(AccessRights::READ_ADD_WRITE);

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == main_purse_read_only
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_as_session_by_hash() {
    let mut builder = setup();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse_read_only = account.main_purse().with_access_rights(AccessRights::READ);

    let exec_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        TRANSFER_AS_SESSION,
        runtime_args! {
            ARG_PURSE => main_purse_read_only,
        },
    )
    .build();

    builder.exec(exec_request).commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == main_purse_read_only.into_read_add_write()
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_main_purse_as_session() {
    let mut builder = setup();

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220204_CALL_CONTRACT,
        runtime_args! {
            ARG_ENTRYPOINT => TRANSFER_MAIN_PURSE_AS_SESSION,
        },
    )
    .build();

    builder.exec(exec_request).commit();

    // This test fails because mint's transfer in a StoredSession is disabled for security reasons
    // introduced as part of EE-1217. This assertion will serve as a reference point when the
    // restriction will be lifted, and this test needs to be updated accordingly.

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::InvalidContext as u8,
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_main_purse_as_session_by_hash() {
    let mut builder = setup();

    let exec_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        TRANSFER_MAIN_PURSE_AS_SESSION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).commit();

    // This test fails because mint's transfer in a StoredSession is disabled for security reasons
    // introduced as part of EE-1217. This assertion will serve as a reference point when the
    // restriction will be lifted, and this test needs to be updated accordingly.

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::InvalidContext as u8,
        ),
        "Expected revert but received {:?}",
        error
    );
}

fn setup() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220204_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(install_request).expect_success().commit();

    builder
}
