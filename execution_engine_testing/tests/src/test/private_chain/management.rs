use std::{collections::BTreeSet, iter::FromIterator};

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    core::{
        engine_state::{engine_config::EngineConfigBuilder, Error},
        execution,
    },
    shared::chain_kind::ChainKind,
};
use casper_types::{
    account::{AccountHash, ActionThresholds, Weight},
    contracts::DEFAULT_ENTRY_POINT_NAME,
    runtime_args,
    system::mint::ADMINISTRATIVE_ACCOUNTS_KEY,
    ApiError, CLValue, RuntimeArgs, StoredValue,
};
use parity_wasm::{
    builder,
    elements::{Instruction, Instructions},
};

use super::{ACCOUNT_1_ADDR, DEFAULT_ADMIN_ACCOUNT_ADDR, DEFAULT_PRIVATE_CHAIN_GENESIS};

const ACCOUNT_MANAGEMENT_CONTRACT: &str = "account_management.wasm";
const ADD_ASSOCIATED_KEY_CONTRACT: &str = "add_associated_key.wasm";
const SET_ACTION_THRESHOLDS_CONTRACT: &str = "set_action_thresholds.wasm";
const UPDATE_ASSOCIATED_KEY_CONTRACT: &str = "update_associated_key.wasm";
const CONTRACT_HASH_NAME: &str = "contract_hash";
const DISABLE_ACCOUNT_ENTRYPOINT: &str = "disable_account";
const ENABLE_ACCOUNT_ENTRYPOINT: &str = "enable_account";
const ARG_ACCOUNT_HASH: &str = "account_hash";

const ARG_ACCOUNT: &str = "account";
const ARG_WEIGHT: &str = "weight";

const ARG_KEY_MANAGEMENT_THRESHOLD: &str = "key_management_threshold";
const ARG_DEPLOY_THRESHOLD: &str = "deploy_threshold";

/// Creates minimal session code that does only one "nop" opcode
pub fn do_minimum_bytes() -> Vec<u8> {
    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
        .with_instructions(Instructions::new(vec![Instruction::Nop, Instruction::End]))
        .build()
        .build()
        // Export above function
        .export()
        .field(DEFAULT_ENTRY_POINT_NAME)
        .build()
        // Memory section is mandatory
        .memory()
        .build()
        .build();
    parity_wasm::serialize(module).expect("should serialize")
}

#[ignore]
#[test]
fn should_not_resolve_private_chain_host_functions_on_public_chain() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        ACCOUNT_MANAGEMENT_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_failure();

    let error = builder.get_error().expect("should have error");

    assert!(matches!(
        error,
        Error::Exec(execution::Error::Interpreter(msg))
        if msg == "host module doesn't export function with name casper_control_management"
    ));
}

#[ignore]
#[test]
fn genesis_accounts_should_not_update_key_weight() {
    let mut builder = setup();

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1");
    assert_eq!(
        account_1.action_thresholds(),
        &ActionThresholds {
            deployment: Weight::new(1),
            key_management: Weight::MAX,
        }
    );

    let exec_request_1 = {
        let session_args = runtime_args! {
            ARG_ACCOUNT => *ACCOUNT_1_ADDR,
            ARG_WEIGHT => Weight::MAX,
        };
        ExecuteRequestBuilder::standard(
            *ACCOUNT_1_ADDR,
            UPDATE_ASSOCIATED_KEY_CONTRACT,
            session_args,
        )
        .build()
    };

    builder.exec(exec_request_1).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            Error::Exec(execution::Error::Revert(ApiError::PermissionDenied))
        ),
        "{:?}",
        error
    );

    let exec_request_2 = {
        let session_args = runtime_args! {
            ARG_ACCOUNT => *DEFAULT_ADMIN_ACCOUNT_ADDR,
            ARG_WEIGHT => Weight::new(1),
        };
        ExecuteRequestBuilder::standard(
            *ACCOUNT_1_ADDR,
            UPDATE_ASSOCIATED_KEY_CONTRACT,
            session_args,
        )
        .build()
    };

    builder.exec(exec_request_2).expect_failure().commit();
}

#[ignore]
#[test]
fn genesis_accounts_should_not_modify_action_thresholds() {
    let mut builder = setup();

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1");
    assert_eq!(
        account_1.action_thresholds(),
        &ActionThresholds {
            deployment: Weight::new(1),
            key_management: Weight::MAX,
        }
    );

    let exec_request = {
        let session_args = runtime_args! {
            ARG_DEPLOY_THRESHOLD => Weight::new(1),
            ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(1),
        };
        ExecuteRequestBuilder::standard(
            *ACCOUNT_1_ADDR,
            SET_ACTION_THRESHOLDS_CONTRACT,
            session_args,
        )
        .build()
    };

    builder.exec(exec_request).expect_failure().commit();
    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            Error::Exec(execution::Error::Revert(ApiError::PermissionDenied))
        ),
        "{:?}",
        error
    );
}

#[ignore]
#[test]
fn genesis_accounts_should_not_manage_their_own_keys() {
    let secondary_account_hash = AccountHash::new([55; 32]);

    let mut builder = setup();

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1");
    assert_eq!(
        account_1.action_thresholds(),
        &ActionThresholds {
            deployment: Weight::new(1),
            key_management: Weight::MAX,
        }
    );

    let exec_request = {
        let session_args = runtime_args! {
            ARG_ACCOUNT => secondary_account_hash,
            ARG_WEIGHT => Weight::MAX,
        };
        ExecuteRequestBuilder::standard(*ACCOUNT_1_ADDR, ADD_ASSOCIATED_KEY_CONTRACT, session_args)
            .build()
    };

    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            Error::Exec(execution::Error::Revert(ApiError::PermissionDenied))
        ),
        "{:?}",
        error
    );
}

#[ignore]
#[test]
fn genesis_accounts_should_have_special_associated_key() {
    let builder = setup();

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should create genesis account");

    let identity_weight = account_1
        .associated_keys()
        .get(&*ACCOUNT_1_ADDR)
        .expect("should have identity key");
    assert_eq!(identity_weight, &Weight::new(1));

    let administrator_account_weight = account_1
        .associated_keys()
        .get(&*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have special account");
    assert_eq!(administrator_account_weight, &Weight::MAX);

    let mint_contract = builder
        .get_contract(builder.get_mint_contract_hash())
        .expect("should create mint");

    let administrative_accounts: Vec<AccountHash> = {
        let administrative_accounts_key = mint_contract
            .named_keys()
            .get(ADMINISTRATIVE_ACCOUNTS_KEY)
            .expect("special accounts should exist");
        let administrative_accounts_stored: StoredValue = builder
            .query(None, *administrative_accounts_key, &[])
            .expect("should query special accounts");
        let administrative_accounts_cl_value: CLValue =
            administrative_accounts_stored.into_clvalue().unwrap();
        administrative_accounts_cl_value.into_t().unwrap()
    };

    assert_eq!(
        BTreeSet::from_iter(administrative_accounts),
        BTreeSet::from_iter([*DEFAULT_ADMIN_ACCOUNT_ADDR])
    );

    let administrator_account = builder
        .get_account(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should create special account");
    assert_eq!(
        administrator_account.associated_keys().len(),
        1,
        "should not have duplicate identity key"
    );

    let identity_weight = administrator_account
        .associated_keys()
        .get(&*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have identity special key");
    assert_eq!(identity_weight, &Weight::new(1));
}

#[ignore]
#[test]
fn administrator_account_should_disable_any_account() {
    let mut builder = setup();

    let account_1_genesis = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1 after genesis");

    // Account 1 can deploy after genesis
    let exec_request_1 = ExecuteRequestBuilder::module_bytes(
        *ACCOUNT_1_ADDR,
        do_minimum_bytes(),
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request_1).expect_success().commit();

    // Freeze account 1
    let freeze_request_1 = {
        let session_args = runtime_args! {
            ARG_ACCOUNT_HASH => *ACCOUNT_1_ADDR,
        };

        ExecuteRequestBuilder::contract_call_by_name(
            *DEFAULT_ADMIN_ACCOUNT_ADDR,
            CONTRACT_HASH_NAME,
            DISABLE_ACCOUNT_ENTRYPOINT,
            session_args,
        )
        .build()
    };

    builder.exec(freeze_request_1).expect_success().commit();
    // Account 1 can not deploy after freezing
    let exec_request_2 = ExecuteRequestBuilder::module_bytes(
        *ACCOUNT_1_ADDR,
        do_minimum_bytes(),
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request_2).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(matches!(
        error,
        Error::Exec(execution::Error::DeploymentAuthorizationFailure)
    ));

    let account_1_frozen = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1 after genesis");
    assert_ne!(
        account_1_genesis, account_1_frozen,
        "account 1 should be modified"
    );

    // Unfreeze account 1
    let unfreeze_request_1 = {
        let session_args = runtime_args! {
            ARG_ACCOUNT_HASH => *ACCOUNT_1_ADDR,
        };

        ExecuteRequestBuilder::contract_call_by_name(
            *DEFAULT_ADMIN_ACCOUNT_ADDR,
            CONTRACT_HASH_NAME,
            ENABLE_ACCOUNT_ENTRYPOINT,
            session_args,
        )
        .build()
    };

    builder.exec(unfreeze_request_1).expect_success().commit();

    // Account 1 can deploy after unfreezing
    let exec_request_3 = ExecuteRequestBuilder::module_bytes(
        *ACCOUNT_1_ADDR,
        do_minimum_bytes(),
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request_3).expect_success().commit();

    let account_1_unfrozen = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1 after genesis");
    assert_eq!(
        account_1_genesis, account_1_unfrozen,
        "account 1 should be modified back to genesis state"
    );
}

fn setup() -> InMemoryWasmTestBuilder {
    let engine_config = EngineConfigBuilder::default()
        .with_chain_kind(ChainKind::Private)
        .build();

    let mut builder = InMemoryWasmTestBuilder::new_with_config(engine_config);
    builder.run_genesis(&DEFAULT_PRIVATE_CHAIN_GENESIS);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        ACCOUNT_MANAGEMENT_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    builder
}
