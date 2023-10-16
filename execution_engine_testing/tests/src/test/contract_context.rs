use assert_matches::assert_matches;
use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution};
use casper_types::addressable_entity::NamedKeys;
use casper_types::{
    package::ENTITY_INITIAL_VERSION, runtime_args, CLType, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, Key, ProtocolVersion, RuntimeArgs, URef,
};

const CONTRACT_HEADERS: &str = "contract_context.wasm";
const PACKAGE_HASH_KEY: &str = "package_hash_key";
const PACKAGE_ACCESS_KEY: &str = "package_access_key";
const CONTRACT_HASH_KEY: &str = "contract_hash_key";
const SESSION_CODE_TEST: &str = "session_code_test";
const CONTRACT_CODE_TEST: &str = "contract_code_test";
const ADD_NEW_KEY_AS_SESSION: &str = "add_new_key_as_session";
const NEW_KEY: &str = "new_key";
const SESSION_CODE_CALLER_AS_CONTRACT: &str = "session_code_caller_as_contract";
const ARG_AMOUNT: &str = "amount";
const CONTRACT_VERSION: &str = "contract_version";

#[ignore]
#[test]
fn should_enforce_intended_execution_contexts() {
    let exec_request_2 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_HASH_KEY,
        Some(ENTITY_INITIAL_VERSION),
        SESSION_CODE_TEST,
        runtime_args! {},
    )
    .build();

    let exec_request_3 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_HASH_KEY,
        Some(ENTITY_INITIAL_VERSION),
        CONTRACT_CODE_TEST,
        runtime_args! {},
    )
    .build();

    let exec_request_4 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_HASH_KEY,
        Some(ENTITY_INITIAL_VERSION),
        ADD_NEW_KEY_AS_SESSION,
        runtime_args! {},
    )
    .build();

    let mut builder = insert_contract_context();

    builder.exec(exec_request_2).expect_failure();

    let expected_error = Error::Exec(execution::Error::InvalidContext);

    builder.assert_error(expected_error);

    builder.exec(exec_request_3).expect_success().commit();

    builder.exec(exec_request_4).expect_failure();

    let expected_error = Error::Exec(execution::Error::InvalidContext);

    builder.assert_error(expected_error);

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

    let _package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    assert!(account.named_keys().get(NEW_KEY).is_none());

    // Check version

    let contract_version_stored = builder
        .query(
            None,
            Key::Account(*DEFAULT_ACCOUNT_ADDR),
            &[CONTRACT_VERSION.to_string()],
        )
        .expect("should query account")
        .as_cl_value()
        .cloned()
        .expect("should be cl value");
    assert_eq!(contract_version_stored.into_t::<u32>().unwrap(), 1u32);
}

#[ignore]
#[test]
fn should_enforce_intended_execution_context_direct_by_name() {
    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_KEY,
        SESSION_CODE_TEST,
        runtime_args! {},
    )
    .build();

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_KEY,
        CONTRACT_CODE_TEST,
        runtime_args! {},
    )
    .build();

    let exec_request_4 = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_KEY,
        ADD_NEW_KEY_AS_SESSION,
        runtime_args! {},
    )
    .build();

    let mut builder = insert_contract_context();

    builder.exec(exec_request_2).expect_failure();

    let expected_error = Error::Exec(execution::Error::InvalidContext);

    builder.assert_error(expected_error);

    builder.exec(exec_request_3).expect_success().commit();

    builder.exec(exec_request_4).expect_failure();

    let expected_error = Error::Exec(execution::Error::InvalidContext);

    builder.assert_error(expected_error);

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

    let _package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    assert!(account.named_keys().get(NEW_KEY).is_none());
}

#[ignore]
#[test]
fn should_enforce_intended_execution_context_direct_by_hash() {
    let mut builder = insert_contract_context();

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let contract_hash = account
        .named_keys()
        .get(CONTRACT_HASH_KEY)
        .expect("should have contract hash")
        .into_entity_hash();

    let contract_hash = contract_hash.unwrap();

    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        SESSION_CODE_TEST,
        runtime_args! {},
    )
    .build();

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CONTRACT_CODE_TEST,
        runtime_args! {},
    )
    .build();

    let exec_request_4 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ADD_NEW_KEY_AS_SESSION,
        runtime_args! {},
    )
    .build();

    builder.exec(exec_request_2).expect_failure();

    builder.assert_error(Error::Exec(execution::Error::InvalidContext));

    builder.exec(exec_request_3).expect_success().commit();

    builder.exec(exec_request_4).expect_failure();

    builder.assert_error(Error::Exec(execution::Error::InvalidContext));

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

    let _package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    assert!(account.named_keys().get(NEW_KEY).is_none())
}

#[ignore]
#[test]
fn should_not_call_session_from_contract() {
    let mut builder = insert_contract_context();

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let contract_package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .cloned()
        .expect("should have contract package");

    let exec_request_2 = {
        let args = runtime_args! {
            PACKAGE_HASH_KEY => contract_package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                SESSION_CODE_CALLER_AS_CONTRACT,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_2).commit();

    let response = builder
        .get_last_exec_result()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

fn create_entrypoints_1() -> EntryPoints {
    let mut entry_points = EntryPoints::new();
    let session_code_test = EntryPoint::new(
        SESSION_CODE_TEST.to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(session_code_test);

    let contract_code_test = EntryPoint::new(
        CONTRACT_CODE_TEST.to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(contract_code_test);

    let session_code_caller_as_session = EntryPoint::new(
        "session_code_caller_as_session".to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(session_code_caller_as_session);

    let session_code_caller_as_contract = EntryPoint::new(
        "session_code_caller_as_contract".to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(session_code_caller_as_contract);

    let add_new_key = EntryPoint::new(
        "add_new_key".to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(add_new_key);
    let add_new_key_as_session = EntryPoint::new(
        "add_new_key_as_session".to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(add_new_key_as_session);

    entry_points
}

fn insert_contract_context() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let contract_named_keys = {
        let contract_variable = URef::default();

        let mut named_keys = NamedKeys::new();
        named_keys.insert("contract_named_key".to_string(), contract_variable.into());
        named_keys
    };

    builder.insert_stored_session(
        create_entrypoints_1(),
        Some(contract_named_keys),
        ProtocolVersion::V1_0_0,
        *DEFAULT_ACCOUNT_ADDR,
        "package_hash_key",
        "contract_hash_key",
    );

    builder
}
