use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state, execution};
use casper_types::{runtime_args, AccessRights, RuntimeArgs};

const REGRESSION_20220204_CONTRACT: &str = "regression_20220204.wasm";
const REGRESSION_20220204_CALL_CONTRACT: &str = "regression_20220204_call.wasm";
const REGRESSION_20220204_NONTRIVIAL_CONTRACT: &str = "regression_20220204_nontrivial.wasm";

const NONTRIVIAL_ARG_AS_CONTRACT: &str = "nontrivial_arg_as_contract";
const ARG_ENTRYPOINT: &str = "entrypoint";
const ARG_PURSE: &str = "purse";
const ARG_NEW_ACCESS_RIGHTS: &str = "new_access_rights";
const TRANSFER_AS_CONTRACT: &str = "transfer_as_contract";

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
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220204_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(install_request).expect_success().commit();

    builder
}
