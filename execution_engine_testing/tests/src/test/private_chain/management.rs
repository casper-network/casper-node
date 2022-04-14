use std::collections::BTreeSet;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_AUCTION_DELAY, DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PAYMENT,
    DEFAULT_PROTOCOL_VERSION, DEFAULT_ROUND_SEIGNIORAGE_RATE, DEFAULT_RUN_GENESIS_REQUEST,
    DEFAULT_SYSTEM_CONFIG, DEFAULT_UNBONDING_DELAY, DEFAULT_VALIDATOR_SLOTS, DEFAULT_WASM_CONFIG,
};
use casper_execution_engine::core::{
    engine_state::{
        engine_config::EngineConfigBuilder, Error, ExecConfig, ExecuteRequest, GenesisAccount,
        RunGenesisRequest,
    },
    execution,
};
use casper_types::{
    account::{AccountHash, ActionThresholds, Weight},
    bytesrepr::ToBytes,
    contracts::DEFAULT_ENTRY_POINT_NAME,
    runtime_args,
    system::{
        auction::{self, DelegationRate},
        mint, standard_payment,
    },
    ApiError, CLType, CLValue, ContractHash, Key, RuntimeArgs, U512,
};
use parity_wasm::{
    builder,
    elements::{Instruction, Instructions},
};

use crate::test::private_chain::{
    ACCOUNT_2_ADDR, ACCOUNT_MANAGEMENT_CONTRACT, ADMIN_1_ACCOUNT_ADDR, ADMIN_1_ACCOUNT_WEIGHT,
    DEFAULT_ADMIN_ACCOUNT_WEIGHT, VALIDATOR_1_PUBLIC_KEY,
};

use super::{
    ACCOUNT_1_ADDR, ACCOUNT_1_PUBLIC_KEY, DEFAULT_ADMIN_ACCOUNT_ADDR,
    PRIVATE_CHAIN_DEFAULT_ACCOUNTS, PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS,
};

const ADD_ASSOCIATED_KEY_CONTRACT: &str = "add_associated_key.wasm";
const SET_ACTION_THRESHOLDS_CONTRACT: &str = "set_action_thresholds.wasm";
const UPDATE_ASSOCIATED_KEY_CONTRACT: &str = "update_associated_key.wasm";
const TRANSFER_TO_ACCOUNT_CONTRACT: &&str = &"transfer_to_account.wasm";
const CONTRACT_HASH_NAME: &str = "contract_hash";
const DISABLE_ACCOUNT_ENTRYPOINT: &str = "disable_account";
const ENABLE_ACCOUNT_ENTRYPOINT: &str = "enable_account";
const ARG_ACCOUNT_HASH: &str = "account_hash";
const ARG_CONTRACT_HASH: &str = "contract_hash";

const ARG_ACCOUNT: &str = "account";
const ARG_WEIGHT: &str = "weight";

const ARG_KEY_MANAGEMENT_THRESHOLD: &str = "key_management_threshold";
const ARG_DEPLOY_THRESHOLD: &str = "deploy_threshold";
const DO_NOTHING_HASH_NAME: &str = "do_nothing_hash";

const DISABLE_CONTRACT_ENTRYPOINT: &str = "disable_contract";
const ENABLE_CONTRACT_ENTRYPOINT: &str = "enable_contract";

const DO_NOTHING_STORED_CONTRACT: &str = "do_nothing_stored.wasm";
const CALL_CONTRACT_PROXY: &str = "call_contract.wasm";
const DELEGATE_ENTRYPOINT: &str = "delegate";

const TEST_PAYMENT_STORED_CONTRACT: &str = "test_payment_stored.wasm";
const TEST_PAYMENT_STORED_HASH_NAME: &str = "test_payment_hash";
const PAY_ENTRYPOINT: &str = "pay";

const EXPECTED_PRIVATE_CHAIN_THRESHOLDS: ActionThresholds = ActionThresholds {
    deployment: Weight::new(1),  // 0 >= 0
    key_management: Weight::MAX, // 0 >= 1
};

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
fn private_chain_genesis_should_have_admin_accounts() {
    assert_eq!(
        super::private_chain_setup()
            .get_engine_state()
            .config()
            .administrative_accounts(),
        &*PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS,
        "private chain genesis has administrator accounts defined"
    );
}

#[ignore]
#[test]
fn public_chain_genesis_should_not_have_admin_accounts() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    assert_eq!(
        builder
            .get_engine_state()
            .config()
            .administrative_accounts(),
        &Vec::new(),
        "private chain genesis has administrator accounts defined"
    );
}

#[should_panic(expected = "DuplicatedAdministratorEntry")]
#[ignore]
#[test]
fn should_not_run_genesis_with_duplicated_administrator_accounts() {
    let engine_config = EngineConfigBuilder::default()
        // This change below makes genesis config validation to fail as administrator accounts are
        // only valid for private chains.
        .with_administrative_accounts(PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS.clone())
        .build();

    let mut builder = InMemoryWasmTestBuilder::new_with_config(engine_config);

    let duplicated_administrator_accounts = {
        let mut accounts = PRIVATE_CHAIN_DEFAULT_ACCOUNTS.clone();

        let genesis_admins = PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS
            .clone()
            .into_iter()
            .map(GenesisAccount::from);
        accounts.extend(genesis_admins);
        accounts
    };

    let genesis_config = ExecConfig::new(
        duplicated_administrator_accounts,
        *DEFAULT_WASM_CONFIG,
        *DEFAULT_SYSTEM_CONFIG,
        DEFAULT_VALIDATOR_SLOTS,
        DEFAULT_AUCTION_DELAY,
        DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        DEFAULT_ROUND_SEIGNIORAGE_RATE,
        DEFAULT_UNBONDING_DELAY,
        DEFAULT_GENESIS_TIMESTAMP_MILLIS,
    );

    let modified_genesis_request = RunGenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        genesis_config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );

    builder.run_genesis(&modified_genesis_request);
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
    let mut builder = super::private_chain_setup();

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1");
    assert_eq!(
        account_1.action_thresholds(),
        &EXPECTED_PRIVATE_CHAIN_THRESHOLDS,
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
    let mut builder = super::private_chain_setup();

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

    let mut builder = super::private_chain_setup();

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
    let builder = super::private_chain_setup();

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should create genesis account");

    let identity_weight = account_1
        .associated_keys()
        .get(&*ACCOUNT_1_ADDR)
        .expect("should have identity key");
    assert_eq!(identity_weight, &Weight::new(1));

    let administrator_key_weight = account_1
        .associated_keys()
        .get(&*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have special account");
    assert_eq!(administrator_key_weight, &DEFAULT_ADMIN_ACCOUNT_WEIGHT);

    let admin_1_key_weight = account_1
        .associated_keys()
        .get(&*ADMIN_1_ACCOUNT_ADDR)
        .expect("should have special account");
    assert_eq!(admin_1_key_weight, &ADMIN_1_ACCOUNT_WEIGHT);

    let administrative_accounts: BTreeSet<AccountHash> = get_administrator_account_hashes(&builder);

    assert!(
        itertools::equal(
            administrative_accounts,
            [*DEFAULT_ADMIN_ACCOUNT_ADDR, *ADMIN_1_ACCOUNT_ADDR]
        ),
        "administrators should be populated with single account"
    );

    let administrator_account = builder
        .get_account(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should create special account");
    assert_eq!(
        administrator_account.associated_keys().len(),
        PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS.len(),
        "should not have duplicate identity key"
    );

    let identity_weight = administrator_account
        .associated_keys()
        .get(&*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have identity special key");
    assert_eq!(identity_weight, &Weight::new(1));
}

fn get_administrator_account_hashes(builder: &InMemoryWasmTestBuilder) -> BTreeSet<AccountHash> {
    builder
        .get_engine_state()
        .config()
        .administrative_accounts()
        .clone()
        .into_iter()
        .map(|admin| admin.public_key().to_account_hash())
        .collect()
}

#[ignore]
#[test]
fn administrator_account_should_disable_any_account() {
    let mut builder = super::private_chain_setup();

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

#[ignore]
#[test]
fn native_transfer_should_create_new_restricted_private_account() {
    let mut builder = super::private_chain_setup();

    // Account 1 can deploy after genesis
    let transfer_args = runtime_args! {
        mint::ARG_TARGET => *ACCOUNT_2_ADDR,
        mint::ARG_AMOUNT => U512::one(),
        mint::ARG_ID => Some(1u64),
    };
    let transfer_request =
        ExecuteRequestBuilder::transfer(*DEFAULT_ADMIN_ACCOUNT_ADDR, transfer_args).build();

    let administrative_accounts = get_administrator_account_hashes(&builder);

    builder.exec(transfer_request).expect_success().commit();

    let account_2 = builder
        .get_account(*ACCOUNT_2_ADDR)
        .expect("should have account 1 after genesis");

    assert!(
        itertools::equal(
            account_2
                .associated_keys()
                .keys()
                .filter(|account_hash| *account_hash != &*ACCOUNT_2_ADDR), // skip identity key
            administrative_accounts.iter(),
        ),
        "newly created account should have administrator accounts set"
    );

    assert_eq!(
        account_2.action_thresholds(),
        &EXPECTED_PRIVATE_CHAIN_THRESHOLDS,
        "newly created account should have expected thresholds"
    );
}

#[ignore]
#[test]
fn wasm_transfer_should_create_new_restricted_private_account() {
    let mut builder = super::private_chain_setup();

    // Account 1 can deploy after genesis
    let transfer_args = runtime_args! {
        mint::ARG_TARGET => *ACCOUNT_2_ADDR,
        mint::ARG_AMOUNT => 1u64,
    };
    let transfer_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        TRANSFER_TO_ACCOUNT_CONTRACT,
        transfer_args,
    )
    .build();

    let administrative_accounts = get_administrator_account_hashes(&builder);

    builder.exec(transfer_request).expect_success().commit();

    let account_2 = builder
        .get_account(*ACCOUNT_2_ADDR)
        .expect("should have account 1 after genesis");

    assert!(
        itertools::equal(
            account_2
                .associated_keys()
                .keys()
                .filter(|account_hash| *account_hash != &*ACCOUNT_2_ADDR), // skip identity key
            administrative_accounts.iter(),
        ),
        "newly created account should have administrator accounts set"
    );

    assert_eq!(
        account_2.action_thresholds(),
        &EXPECTED_PRIVATE_CHAIN_THRESHOLDS,
        "newly created account should have expected thresholds"
    );
}

#[ignore]
#[test]
fn administrator_account_should_disable_any_contract_used_as_session() {
    let mut builder = super::private_chain_setup();

    let store_contract_request = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        DO_NOTHING_STORED_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(store_contract_request)
        .expect_success()
        .commit();

    let account_1_genesis = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1 after genesis");

    let stored_contract_key = account_1_genesis.named_keys()[DO_NOTHING_HASH_NAME];
    let stored_contract_hash = stored_contract_key
        .into_hash()
        .map(ContractHash::new)
        .expect("should have stored contract hash");

    let do_nothing_contract_package_key = {
        let stored_value = builder
            .query(None, stored_contract_key, &[])
            .expect("should query");
        let contract = stored_value.into_contract().expect("should be contract");
        Key::from(contract.contract_package_hash())
    };

    let contract_package_before = builder
        .query(None, do_nothing_contract_package_key, &[])
        .expect("should query")
        .into_contract_package()
        .expect("should be contract package");
    assert_eq!(
        contract_package_before.is_contract_disabled(&stored_contract_hash),
        Some(false),
        "newly stored contract should be enabled"
    );

    // Account 1 can deploy after genesis
    let exec_request_1 = ExecuteRequestBuilder::contract_call_by_name(
        *ACCOUNT_1_ADDR,
        DO_NOTHING_HASH_NAME,
        DELEGATE_ENTRYPOINT,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request_1).expect_success().commit();

    // Freeze account 1
    let disable_request = {
        let session_args = runtime_args! {
            ARG_CONTRACT_HASH => stored_contract_hash,
        };

        ExecuteRequestBuilder::contract_call_by_name(
            *DEFAULT_ADMIN_ACCOUNT_ADDR,
            CONTRACT_HASH_NAME,
            DISABLE_CONTRACT_ENTRYPOINT,
            session_args,
        )
        .build()
    };

    builder.exec(disable_request).expect_success().commit();

    let contract_package_after_disable = builder
        .query(None, do_nothing_contract_package_key, &[])
        .expect("should query")
        .into_contract_package()
        .expect("should be contract package");

    assert_ne!(
        contract_package_before, contract_package_after_disable,
        "contract package should be disabled"
    );
    assert_eq!(
        contract_package_after_disable.is_contract_disabled(&stored_contract_hash),
        Some(true)
    );

    let call_delegate_requests = {
        // Unable to call disabled stored contract directly
        let call_delegate_by_name = ExecuteRequestBuilder::contract_call_by_name(
            *ACCOUNT_1_ADDR,
            DO_NOTHING_HASH_NAME,
            DELEGATE_ENTRYPOINT,
            RuntimeArgs::default(),
        )
        .build();

        let call_delegate_by_hash = ExecuteRequestBuilder::contract_call_by_hash(
            *ACCOUNT_1_ADDR,
            stored_contract_hash,
            DELEGATE_ENTRYPOINT,
            RuntimeArgs::default(),
        )
        .build();

        let call_delegate_from_wasm = make_call_contract_session_request(
            *ACCOUNT_1_ADDR,
            stored_contract_hash,
            DELEGATE_ENTRYPOINT,
            RuntimeArgs::default(),
        );

        vec![
            call_delegate_by_name,
            call_delegate_by_hash,
            call_delegate_from_wasm,
        ]
    };

    for call_delegate_request in call_delegate_requests {
        builder
            .exec(call_delegate_request)
            .expect_failure()
            .commit();
        let error = builder.get_error().expect("should have error");
        assert!(
            matches!(
                error,
                Error::Exec(execution::Error::DisabledContract(disabled_contract_hash))
                if disabled_contract_hash == stored_contract_hash
            ),
            "expected disabled contract error, found {:?}",
            error
        );
    }

    // Enable contract
    let enable_contract_request = {
        let session_args = runtime_args! {
            ARG_CONTRACT_HASH => stored_contract_hash,
        };

        ExecuteRequestBuilder::contract_call_by_name(
            *DEFAULT_ADMIN_ACCOUNT_ADDR,
            CONTRACT_HASH_NAME,
            ENABLE_CONTRACT_ENTRYPOINT,
            session_args,
        )
        .build()
    };

    builder
        .exec(enable_contract_request)
        .expect_success()
        .commit();

    let contract_package_after_enable = builder
        .query(None, do_nothing_contract_package_key, &[])
        .expect("should query")
        .into_contract_package()
        .expect("should be contract package");

    assert_eq!(
        contract_package_before, contract_package_after_enable,
        "enabling a contract should remove entries from disabled versions"
    );
    assert_eq!(
        contract_package_after_enable.is_contract_disabled(&stored_contract_hash),
        Some(false)
    );

    // Account 1 can deploy after enabling
    let call_delegate_requests_2 = {
        // Unable to call disabled stored contract directly
        let call_delegate_by_name = ExecuteRequestBuilder::contract_call_by_name(
            *ACCOUNT_1_ADDR,
            DO_NOTHING_HASH_NAME,
            DELEGATE_ENTRYPOINT,
            RuntimeArgs::default(),
        )
        .build();

        let call_delegate_by_hash = ExecuteRequestBuilder::contract_call_by_hash(
            *ACCOUNT_1_ADDR,
            stored_contract_hash,
            DELEGATE_ENTRYPOINT,
            RuntimeArgs::default(),
        )
        .build();

        let call_delegate_from_wasm = make_call_contract_session_request(
            *ACCOUNT_1_ADDR,
            stored_contract_hash,
            DELEGATE_ENTRYPOINT,
            RuntimeArgs::default(),
        );

        vec![
            call_delegate_by_name,
            call_delegate_by_hash,
            call_delegate_from_wasm,
        ]
    };

    for call_delegate_request in call_delegate_requests_2 {
        builder
            .exec(call_delegate_request)
            .expect_success()
            .commit();
    }
}

#[ignore]
#[test]
fn administrator_account_should_disable_any_contract_used_as_payment() {
    let mut builder = super::private_chain_setup();

    let store_contract_request = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        TEST_PAYMENT_STORED_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(store_contract_request)
        .expect_success()
        .commit();

    let account_1_genesis = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1 after genesis");

    let stored_contract_key = account_1_genesis.named_keys()[TEST_PAYMENT_STORED_HASH_NAME];
    let stored_contract_hash = stored_contract_key
        .into_hash()
        .map(ContractHash::new)
        .expect("should have stored contract hash");

    let test_payment_stored_package_key = {
        let stored_value = builder
            .query(None, stored_contract_key, &[])
            .expect("should query");
        let contract = stored_value.into_contract().expect("should be contract");
        Key::from(contract.contract_package_hash())
    };

    let contract_package_before = builder
        .query(None, test_payment_stored_package_key, &[])
        .expect("should query")
        .into_contract_package()
        .expect("should be contract package");
    assert_eq!(
        contract_package_before.is_contract_disabled(&stored_contract_hash),
        Some(false),
        "newly stored contract should be enabled"
    );

    // Account 1 can deploy after genesis
    let exec_request_1 = {
        let sender = *ACCOUNT_1_ADDR;
        let deploy_hash = [100; 32];

        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => *DEFAULT_PAYMENT,
        };
        let session_args = RuntimeArgs::default();

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_session_bytes(do_minimum_bytes(), session_args)
            .with_stored_payment_named_key(
                TEST_PAYMENT_STORED_HASH_NAME,
                PAY_ENTRYPOINT,
                payment_args,
            )
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();
        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_1).expect_success().commit();

    // Disable payment contract
    let disable_request = {
        let session_args = runtime_args! {
            ARG_CONTRACT_HASH => stored_contract_hash,
        };

        ExecuteRequestBuilder::contract_call_by_name(
            *DEFAULT_ADMIN_ACCOUNT_ADDR,
            CONTRACT_HASH_NAME,
            DISABLE_CONTRACT_ENTRYPOINT,
            session_args,
        )
        .build()
    };

    builder.exec(disable_request).expect_success().commit();

    let contract_package_after_disable = builder
        .query(None, test_payment_stored_package_key, &[])
        .expect("should query")
        .into_contract_package()
        .expect("should be contract package");

    assert_ne!(
        contract_package_before, contract_package_after_disable,
        "contract package should be disabled"
    );
    assert_eq!(
        contract_package_after_disable.is_contract_disabled(&stored_contract_hash),
        Some(true)
    );

    let call_stored_payment_requests_1 = {
        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => *DEFAULT_PAYMENT,
        };
        let session_args = RuntimeArgs::default();

        let call_by_name = {
            let sender = *ACCOUNT_1_ADDR;
            let deploy_hash = [100; 32];

            let deploy = DeployItemBuilder::new()
                .with_address(sender)
                .with_session_bytes(do_minimum_bytes(), session_args.clone())
                .with_stored_payment_named_key(
                    TEST_PAYMENT_STORED_HASH_NAME,
                    PAY_ENTRYPOINT,
                    payment_args.clone(),
                )
                .with_authorization_keys(&[sender])
                .with_deploy_hash(deploy_hash)
                .build();
            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        let call_by_hash = {
            let sender = *ACCOUNT_1_ADDR;
            let deploy_hash = [100; 32];

            let deploy = DeployItemBuilder::new()
                .with_address(sender)
                .with_session_bytes(do_minimum_bytes(), session_args)
                .with_stored_payment_hash(stored_contract_hash, PAY_ENTRYPOINT, payment_args)
                .with_authorization_keys(&[sender])
                .with_deploy_hash(deploy_hash)
                .build();
            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        vec![call_by_name, call_by_hash]
    };

    for execute_request in call_stored_payment_requests_1 {
        builder.exec(execute_request).expect_failure().commit();
        let error = builder.get_error().expect("should have error");
        assert!(
            matches!(
                error,
                Error::Exec(execution::Error::DisabledContract(disabled_contract_hash))
                if disabled_contract_hash == stored_contract_hash
            ),
            "expected disabled contract error, found {:?}",
            error
        );
    }

    // Enable contract
    let enable_contract_request = {
        let session_args = runtime_args! {
            ARG_CONTRACT_HASH => stored_contract_hash,
        };

        ExecuteRequestBuilder::contract_call_by_name(
            *DEFAULT_ADMIN_ACCOUNT_ADDR,
            CONTRACT_HASH_NAME,
            ENABLE_CONTRACT_ENTRYPOINT,
            session_args,
        )
        .build()
    };

    builder
        .exec(enable_contract_request)
        .expect_success()
        .commit();

    let contract_package_after_enable = builder
        .query(None, test_payment_stored_package_key, &[])
        .expect("should query")
        .into_contract_package()
        .expect("should be contract package");

    assert_eq!(
        contract_package_before, contract_package_after_enable,
        "enabling a contract should remove entries from disabled versions"
    );
    assert_eq!(
        contract_package_after_enable.is_contract_disabled(&stored_contract_hash),
        Some(false)
    );

    // Account 1 can deploy after enabling

    let call_stored_payment_requests_2 = {
        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => *DEFAULT_PAYMENT,
        };
        let session_args = RuntimeArgs::default();

        let call_by_name = {
            let sender = *ACCOUNT_1_ADDR;
            let deploy_hash = [100; 32];

            let deploy = DeployItemBuilder::new()
                .with_address(sender)
                .with_session_bytes(do_minimum_bytes(), session_args)
                .with_stored_payment_named_key(
                    TEST_PAYMENT_STORED_HASH_NAME,
                    PAY_ENTRYPOINT,
                    payment_args.clone(),
                )
                .with_authorization_keys(&[sender])
                .with_deploy_hash(deploy_hash)
                .build();
            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        let call_by_hash = {
            let sender = *ACCOUNT_1_ADDR;
            let deploy_hash = [100; 32];

            let session_args = RuntimeArgs::default();

            let deploy = DeployItemBuilder::new()
                .with_address(sender)
                .with_session_bytes(do_minimum_bytes(), session_args)
                .with_stored_payment_hash(stored_contract_hash, PAY_ENTRYPOINT, payment_args)
                .with_authorization_keys(&[sender])
                .with_deploy_hash(deploy_hash)
                .build();
            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        vec![call_by_name, call_by_hash]
    };

    for execute_request in call_stored_payment_requests_2 {
        builder.exec(execute_request).expect_success().commit();
    }
}

#[ignore]
#[test]
fn should_not_allow_add_bid_on_private_chain() {
    let mut builder = super::private_chain_setup();

    let delegation_rate: DelegationRate = 4;
    let session_args = runtime_args! {
        auction::ARG_PUBLIC_KEY => ACCOUNT_1_PUBLIC_KEY.clone(),
        auction::ARG_AMOUNT => U512::one(),
        auction::ARG_DELEGATION_RATE => delegation_rate,
    };

    let exec_request =
        ExecuteRequestBuilder::standard(*ACCOUNT_1_ADDR, "add_bid.wasm", session_args).build();

    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().expect("should have error");

    assert!(matches!(
        error,
        Error::Exec(execution::Error::Revert(api_error))
        if api_error == auction::Error::AuctionBidsDisabled.into()
    ));
}

#[ignore]
#[test]
fn should_not_allow_delegate_on_private_chain() {
    let mut builder = super::private_chain_setup();

    let session_args = runtime_args! {
        auction::ARG_DELEGATOR => ACCOUNT_1_PUBLIC_KEY.clone(),
        auction::ARG_VALIDATOR => VALIDATOR_1_PUBLIC_KEY.clone(),
        auction::ARG_AMOUNT => U512::one(),
    };

    let exec_request =
        ExecuteRequestBuilder::standard(*ACCOUNT_1_ADDR, "delegate.wasm", session_args).build();

    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().expect("should have error");

    assert!(
        matches!(
            error,
            Error::Exec(execution::Error::Revert(api_error))
            if api_error == auction::Error::AuctionBidsDisabled.into()
        ),
        "{:?}",
        error
    );
    // Redelegation would not work since delegate, and add_bid are disabled on private chains
    // therefore there is nothing to test.
}

fn make_call_contract_session_request(
    account_hash: AccountHash,
    contract_hash: ContractHash,
    entrypoint: &str,
    arguments: RuntimeArgs,
) -> ExecuteRequest {
    let arguments_any = {
        let arg_bytes = arguments.to_bytes().unwrap();
        CLValue::from_components(CLType::Any, arg_bytes)
    };

    let mut session_args = runtime_args! {
        "entrypoint" => entrypoint,
        "contract_hash" => contract_hash,
    };
    session_args.insert_cl_value("arguments", arguments_any);

    ExecuteRequestBuilder::standard(account_hash, CALL_CONTRACT_PROXY, session_args).build()
}
