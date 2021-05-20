use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    AccountHash, DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::{engine_state::Error as EngineError, execution::Error};
use casper_types::{
    bytesrepr::ToBytes, runtime_args, system::mint, AccessRights, CLType, ContractHash, Key,
    RuntimeArgs, U512,
};
use local_state_call::{NEW_LOCAL_KEY_NAME, NEW_LOCAL_KEY_VALUE};

const LOCAL_STATE_WASM: &str = "local_state.wasm";
const LOCAL_STATE_CALL_WASM: &str = "local_state_call.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);

fn setup() -> (InMemoryWasmTestBuilder, ContractHash) {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let fund_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    let install_contract_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        LOCAL_STATE_WASM,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(fund_request).commit().expect_success();

    builder
        .exec(install_contract_request)
        .commit()
        .expect_success();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");
    let contract_hash = account
        .named_keys()
        .get(local_state::CONTRACT_HASH_NAME)
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("should have hash");

    (builder, contract_hash)
}

#[ignore]
#[test]
fn should_modify_with_owned_access_rights() {
    let (mut builder, contract_hash) = setup();

    let modify_write_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        local_state::MODIFY_WRITE_ENTRYPOINT,
        RuntimeArgs::default(),
    )
    .build();
    let modify_write_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        local_state::MODIFY_WRITE_ENTRYPOINT,
        RuntimeArgs::default(),
    )
    .build();

    let contract = builder
        .get_contract(contract_hash)
        .expect("should have account");

    let stored_local_key = contract
        .named_keys()
        .get(local_state::LOCAL_KEY_NAME)
        .expect("local key");
    let local_key_root_uref = stored_local_key.into_uref().expect("should be uref");

    let key_bytes = local_state::WRITE_LOCAL_KEY.to_bytes().unwrap();
    let local_key_value = Key::local(local_key_root_uref, &key_bytes);

    builder
        .exec(modify_write_request_1)
        .commit()
        .expect_success();

    let stored_value = builder
        .query(None, local_key_root_uref.into(), &[])
        .expect("should have value");
    let local_uref_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should have cl value");
    assert_eq!(
        local_uref_value.cl_type(),
        &CLType::Unit,
        "created local key uref should be unit"
    );

    let stored_value = builder
        .query(None, local_key_value, &[])
        .expect("should have value");
    let local_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should have cl value");

    let s: String = local_value.into_t().expect("should be a string");
    assert_eq!(s, "Hello, world!");

    builder
        .exec(modify_write_request_2)
        .commit()
        .expect_success();

    let stored_value = builder
        .query(None, local_key_value, &[])
        .expect("should have value");
    let local_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should have cl value");

    let s: String = local_value.into_t().expect("should be a string");
    assert_eq!(s, "Hello, world! Hello, world!");
}

#[ignore]
#[test]
fn should_not_write_with_ro_access_rights() {
    let (mut builder, contract_hash) = setup();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        LOCAL_STATE_CALL_WASM,
        runtime_args! {
            local_state_call::ARG_OPERATION => local_state_call::OP_WRITE,
            local_state_call::ARG_SHARE_UREF_ENTRYPOINT => local_state::SHARE_RO_ENTRYPOINT,
            local_state_call::ARG_CONTRACT_HASH => contract_hash,
        },
    )
    .build();

    builder.exec(call_request).commit();

    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::InvalidAccess {
                required: AccessRights::WRITE
            })
        ),
        "Received error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_read_with_ro_access_rights() {
    let (mut builder, contract_hash) = setup();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        LOCAL_STATE_CALL_WASM,
        runtime_args! {
            local_state_call::ARG_OPERATION => local_state_call::OP_READ,
            local_state_call::ARG_SHARE_UREF_ENTRYPOINT => local_state::SHARE_RO_ENTRYPOINT,
            local_state_call::ARG_CONTRACT_HASH => contract_hash,
        },
    )
    .build();

    builder.exec(call_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_not_read_with_write_access_rights() {
    let (mut builder, contract_hash) = setup();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        LOCAL_STATE_CALL_WASM,
        runtime_args! {
            local_state_call::ARG_OPERATION => local_state_call::OP_READ,
            local_state_call::ARG_SHARE_UREF_ENTRYPOINT => local_state::SHARE_W_ENTRYPOINT,
            local_state_call::ARG_CONTRACT_HASH => contract_hash,
        },
    )
    .build();

    builder.exec(call_request).commit();

    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::InvalidAccess {
                required: AccessRights::READ
            })
        ),
        "Received error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_write_with_write_access_rights() {
    let (mut builder, contract_hash) = setup();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        LOCAL_STATE_CALL_WASM,
        runtime_args! {
            local_state_call::ARG_OPERATION => local_state_call::OP_WRITE,
            local_state_call::ARG_SHARE_UREF_ENTRYPOINT => local_state::SHARE_W_ENTRYPOINT,
            local_state_call::ARG_CONTRACT_HASH => contract_hash,
        },
    )
    .build();

    builder.exec(call_request).commit();

    let contract = builder
        .get_contract(contract_hash)
        .expect("should have account");

    let stored_local_key = contract
        .named_keys()
        .get(local_state::LOCAL_KEY_NAME)
        .expect("local key");
    let local_key_root_uref = stored_local_key.into_uref().expect("should be uref");

    let local_key = Key::local(local_key_root_uref, &NEW_LOCAL_KEY_NAME.to_bytes().unwrap());

    let result = builder.query(None, local_key, &[]).expect("should query");
    let cl_value = result.as_cl_value().cloned().expect("should have cl value");
    let written_value: String = cl_value.into_t().expect("should get string");
    assert_eq!(written_value, NEW_LOCAL_KEY_VALUE);
}

#[ignore]
#[test]
fn should_not_write_with_forged_uref() {
    let (mut builder, contract_hash) = setup();

    let contract = builder
        .get_contract(contract_hash)
        .expect("should have account");

    let stored_local_key = contract
        .named_keys()
        .get(local_state::LOCAL_KEY_NAME)
        .expect("local key");
    let local_key_root_uref = stored_local_key.into_uref().expect("should be uref");

    // Do some extra forging on the uref
    let forged_uref = local_key_root_uref.into_read_add_write();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        LOCAL_STATE_CALL_WASM,
        runtime_args! {
            local_state_call::ARG_OPERATION => local_state_call::OP_FORGED_UREF_WRITE,
            local_state_call::ARG_FORGED_UREF => forged_uref,
        },
    )
    .build();

    builder.exec(call_request).commit();

    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::ForgedReference(uref))
            if *uref == forged_uref
        ),
        "Received error {:?}",
        error
    );
}
