use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{engine_state, execution};
use casper_types::{runtime_args, system::mint, AccessRights, ApiError, RuntimeArgs};

const REGRESSION_20220204_CONTRACT: &str = "regression_20220204.wasm";
const REGRESSION_20220204_CALL_CONTRACT: &str = "regression_20220204_call.wasm";
const REGRESSION_20220204_NONTRIVIAL_CONTRACT: &str = "regression_20220204_nontrivial.wasm";
const TRANSFER_MAIN_PURSE_AS_SESSION: &str = "transfer_main_purse_as_session";
const NONTRIVIAL_ARG_AS_CONTRACT: &str = "nontrivial_arg_as_contract";
const ARG_ENTRYPOINT: &str = "entrypoint";
const ARG_PURSE: &str = "purse";
const ARG_NEW_ACCESS_RIGHTS: &str = "new_access_rights";
const TRANSFER_AS_CONTRACT: &str = "transfer_as_contract";
const TRANSFER_AS_SESSION: &str = "transfer_as_session";
const CONTRACT_HASH_NAME: &str = "regression-contract-hash";

#[ignore]
#[test]
fn regression_20220204_as_contract() {
    let contract = REGRESSION_20220204_CALL_CONTRACT;
    let entrypoint = TRANSFER_AS_CONTRACT;
    let new_access_rights = AccessRights::READ_ADD_WRITE;
    let expected = AccessRights::READ_ADD_WRITE;
    let mut builder = setup();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        contract,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
            ARG_ENTRYPOINT => entrypoint,
        },
    )
    .build();
    builder.exec(exec_request_2).commit();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse().with_access_rights(expected);
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
fn regression_20220204_as_contract_attenuated() {
    let contract = REGRESSION_20220204_CALL_CONTRACT;
    let entrypoint = TRANSFER_AS_CONTRACT;
    let new_access_rights = AccessRights::READ;
    let expected = AccessRights::READ_ADD_WRITE;
    let mut builder = setup();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        contract,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
            ARG_ENTRYPOINT => entrypoint,
        },
    )
    .build();
    builder.exec(exec_request_2).commit();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse().with_access_rights(expected);
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
    let contract = REGRESSION_20220204_CALL_CONTRACT;
    let entrypoint = TRANSFER_AS_CONTRACT;
    let new_access_rights = AccessRights::WRITE;
    let expected = AccessRights::READ_ADD_WRITE;
    let mut builder = setup();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        contract,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
            ARG_ENTRYPOINT => entrypoint,
        },
    )
    .build();
    builder.exec(exec_request_2).commit();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse().with_access_rights(expected);
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
    let entrypoint = TRANSFER_AS_CONTRACT;
    let new_access_rights = AccessRights::READ_ADD_WRITE;
    let expected = AccessRights::READ_ADD_WRITE;
    let mut builder = setup();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse();
    let exec_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        entrypoint,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
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
            if forged_uref == main_purse.with_access_rights(expected)
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_nontrivial_arg_as_contract() {
    let contract = REGRESSION_20220204_NONTRIVIAL_CONTRACT;
    let entrypoint = NONTRIVIAL_ARG_AS_CONTRACT;
    let new_access_rights = AccessRights::READ_ADD_WRITE;
    let expected = AccessRights::READ_ADD_WRITE;
    let mut builder = setup();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        contract,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
            ARG_ENTRYPOINT => entrypoint,
        },
    )
    .build();
    builder.exec(exec_request_2).commit();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse().with_access_rights(expected);
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
fn regression_20220204_as_contract_by_hash_attenuated() {
    let entrypoint = TRANSFER_AS_CONTRACT;
    let new_access_rights = AccessRights::READ;
    let expected = AccessRights::READ_ADD_WRITE;
    let mut builder = setup();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse();
    let exec_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        entrypoint,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
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
            if forged_uref == main_purse.with_access_rights(expected)
        ),
        "Expected revert but received {:?}",
        error
    );
    let entrypoint = TRANSFER_AS_CONTRACT;
    let new_access_rights = AccessRights::WRITE;
    let expected = AccessRights::READ_ADD_WRITE;
    let mut builder = setup();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse();
    let exec_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        entrypoint,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
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
            if forged_uref == main_purse.with_access_rights(expected)
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_as_session() {
    // Demonstrates that a stored session cannot transfer funds.
    let entrypoint = TRANSFER_AS_SESSION;
    let new_access_rights = AccessRights::READ_ADD_WRITE;
    let mut builder = setup();
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220204_CALL_CONTRACT,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
            ARG_ENTRYPOINT => entrypoint,
        },
    )
    .build();
    builder.exec(exec_request_1).commit();
    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::InvalidContext as u8
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_as_session_attenuated() {
    let contract = REGRESSION_20220204_CALL_CONTRACT;
    let entrypoint = TRANSFER_AS_SESSION;
    let new_access_rights = AccessRights::ADD;
    let mut builder = setup();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        contract,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
            ARG_ENTRYPOINT => entrypoint,
        },
    )
    .build();
    builder.exec(exec_request_2).commit();
    let error = builder.get_error().expect("should have returned an error");
    println!("{:?}", error);
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::InvalidContext as u8
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_as_session_by_hash() {
    let entrypoint = TRANSFER_AS_SESSION;
    let new_access_rights = AccessRights::READ_ADD_WRITE;
    let mut builder = setup();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse();
    let exec_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        entrypoint,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
            ARG_PURSE => main_purse,
        },
    )
    .build();
    builder.exec(exec_request).commit();
    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::InvalidContext as u8
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220204_as_session_by_hash_attenuated() {
    let entrypoint = TRANSFER_AS_SESSION;
    let new_access_rights = AccessRights::READ;
    let mut builder = setup();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse();
    let exec_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        entrypoint,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
            ARG_PURSE => main_purse,
        },
    )
    .build();
    builder.exec(exec_request).commit();
    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::InvalidContext as u8
        ),
        "Expected revert but received {:?}",
        error
    );
    let entrypoint = TRANSFER_AS_SESSION;
    let new_access_rights = AccessRights::ADD;
    let mut builder = setup();
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let main_purse = account.main_purse();
    let exec_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        entrypoint,
        runtime_args! {
            ARG_NEW_ACCESS_RIGHTS => new_access_rights.bits(),
            ARG_PURSE => main_purse,
        },
    )
    .build();
    builder.exec(exec_request).commit();
    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::InvalidContext as u8
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
            ARG_NEW_ACCESS_RIGHTS => AccessRights::READ_ADD_WRITE.bits(),
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
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220204_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(install_request).expect_success().commit();

    builder
}
