use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder, DEFAULT_ACCOUNT_ADDR,
    MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};

use casper_execution_engine::{engine_state, execution::Error};
use casper_types::{
    account::AccountHash,
    addressable_entity::{AssociatedKeys, Weight},
    package::{EntityVersion, ENTITY_INITIAL_VERSION},
    runtime_args,
    system::mint,
    AddressableEntityHash, CLValue, EraId, PackageHash, ProtocolVersion, RuntimeArgs, StoredValue,
};

const DO_NOTHING_STORED_CONTRACT_NAME: &str = "do_nothing_stored";
const DO_NOTHING_STORED_UPGRADER_CONTRACT_NAME: &str = "do_nothing_stored_upgrader";
const DO_NOTHING_STORED_CALLER_CONTRACT_NAME: &str = "do_nothing_stored_caller";
const PURSE_HOLDER_STORED_CALLER_CONTRACT_NAME: &str = "purse_holder_stored_caller";
const PURSE_HOLDER_STORED_CONTRACT_NAME: &str = "purse_holder_stored";
const PURSE_HOLDER_STORED_UPGRADER_CONTRACT_NAME: &str = "purse_holder_stored_upgrader";
const UPGRADE_THRESHOLD_CONTRACT_NAME: &str = "upgrade_threshold.wasm";
const UPGRADE_THRESHOLD_UPGRADER: &str = "upgrade_threshold_upgrader.wasm";

const ENTRY_FUNCTION_NAME: &str = "delegate";
const DO_NOTHING_CONTRACT_NAME: &str = "do_nothing_package_hash";
const DO_NOTHING_HASH_KEY_NAME: &str = "do_nothing_hash";

const INITIAL_VERSION: EntityVersion = ENTITY_INITIAL_VERSION;
const UPGRADED_VERSION: EntityVersion = INITIAL_VERSION + 1;
const PURSE_NAME_ARG_NAME: &str = "purse_name";
const PURSE_1: &str = "purse_1";
const METHOD_REMOVE: &str = "remove";
const VERSION: &str = "version";

const HASH_KEY_NAME: &str = "purse_holder";

const TOTAL_PURSES: usize = 3;
const PURSE_NAME: &str = "purse_name";
const ENTRY_POINT_NAME: &str = "entry_point";
const ENTRY_POINT_ADD: &str = "add_named_purse";
const ARG_CONTRACT_PACKAGE: &str = "contract_package";
const ARG_VERSION: &str = "version";
const ARG_NEW_PURSE_NAME: &str = "new_purse_name";
const ARG_IS_LOCKED: &str = "is_locked";

/// Performs define and execution of versioned contracts, calling them directly from hash
#[ignore]
#[test]
fn should_upgrade_do_nothing_to_do_something_version_hash_call() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // Create contract package and store contract ver: 1.0.0 with "delegate" entry function
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", DO_NOTHING_STORED_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                RuntimeArgs::default(),
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    // Calling initial version from contract package hash, should have no effects
    {
        let exec_request = {
            ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                DO_NOTHING_CONTRACT_NAME,
                Some(INITIAL_VERSION),
                ENTRY_FUNCTION_NAME,
                RuntimeArgs::new(),
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let account_1 = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let entity_hash = account_1
        .named_keys()
        .get(DO_NOTHING_HASH_KEY_NAME)
        .expect("must have do-nothing-hash")
        .into_entity_hash()
        .unwrap();

    let entity = builder
        .get_entity_with_named_keys_by_entity_hash(entity_hash)
        .expect("must have entity");

    assert!(
        entity.named_keys().get(PURSE_1).is_none(),
        "purse should not exist",
    );

    // Upgrade version having call to create_purse_01
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", DO_NOTHING_STORED_UPGRADER_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                RuntimeArgs::default(),
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    // Calling upgraded version, expecting purse creation
    {
        let args = runtime_args! {
            PURSE_NAME_ARG_NAME => PURSE_1,
        };
        let exec_request = {
            ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                DO_NOTHING_CONTRACT_NAME,
                Some(UPGRADED_VERSION),
                ENTRY_FUNCTION_NAME,
                args,
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let account_1 = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let entity_hash = account_1
        .named_keys()
        .get("end of upgrade")
        .expect("must have do-nothing-hash")
        .into_entity_hash()
        .unwrap();

    let entity = builder
        .get_entity_with_named_keys_by_entity_hash(entity_hash)
        .expect("must have entity");

    assert!(
        entity.named_keys().get(PURSE_1).is_some(),
        "purse should exist",
    );
}

/// Performs define and execution of versioned contracts, calling them from a contract
#[ignore]
#[test]
fn should_upgrade_do_nothing_to_do_something_contract_call() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // Create contract package and store contract ver: 1.0.0
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", DO_NOTHING_STORED_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                RuntimeArgs::default(),
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let account_1 = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    account_1
        .named_keys()
        .get(DO_NOTHING_HASH_KEY_NAME)
        .expect("should have key of do_nothing_hash")
        .into_entity_hash_addr()
        .expect("should have into hash");

    let stored_contract_package_hash = account_1
        .named_keys()
        .get(DO_NOTHING_CONTRACT_NAME)
        .expect("should have key of do_nothing_hash")
        .into_package_addr()
        .expect("should have hash");

    // Calling initial stored version from contract package hash, should have no effects
    {
        let contract_name = format!("{}.wasm", DO_NOTHING_STORED_CALLER_CONTRACT_NAME);
        let args = runtime_args! {
            ARG_CONTRACT_PACKAGE => stored_contract_package_hash,
            ARG_VERSION => INITIAL_VERSION,
            ARG_NEW_PURSE_NAME => PURSE_1,
        };
        let exec_request = {
            ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, &contract_name, args).build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let account_1 = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let entity_hash = account_1
        .named_keys()
        .get(DO_NOTHING_HASH_KEY_NAME)
        .expect("must have do-nothing-hash")
        .into_entity_hash()
        .unwrap();

    let entity = builder
        .get_entity_with_named_keys_by_entity_hash(entity_hash)
        .expect("must have entity");

    assert!(
        entity.named_keys().get(PURSE_1).is_none(),
        "purse should not exist",
    );

    // Upgrade stored contract to version: 2.0.0, having call to create_purse_01
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", DO_NOTHING_STORED_UPGRADER_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                RuntimeArgs::default(),
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let stored_contract_package_hash = account_1
        .named_keys()
        .get(DO_NOTHING_CONTRACT_NAME)
        .expect("should have key of do_nothing_hash")
        .into_package_addr()
        .expect("should have hash");

    // Calling upgraded stored version, expecting purse creation
    {
        let contract_name = format!("{}.wasm", DO_NOTHING_STORED_CALLER_CONTRACT_NAME);
        let args = runtime_args! {
            ARG_CONTRACT_PACKAGE => stored_contract_package_hash,
            ARG_VERSION => UPGRADED_VERSION,
            ARG_NEW_PURSE_NAME => PURSE_1,
        };

        let exec_request = {
            ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, &contract_name, args).build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let account_1 = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let entity_hash = account_1
        .named_keys()
        .get("end of upgrade")
        .expect("must have do-nothing-hash")
        .into_entity_hash()
        .unwrap();

    let entity = builder
        .get_entity_with_named_keys_by_entity_hash(entity_hash)
        .expect("must have entity");

    assert!(
        entity.named_keys().get(PURSE_1).is_some(),
        "purse should exist",
    );
}

#[ignore]
#[test]
fn should_be_able_to_observe_state_transition_across_upgrade() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // store do-nothing-stored
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    ARG_IS_LOCKED => false,
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    assert!(
        account.named_keys().contains(VERSION),
        "version uref should exist on install"
    );

    let stored_package_hash: PackageHash = account
        .named_keys()
        .get(HASH_KEY_NAME)
        .expect("should have stored uref")
        .into_package_addr()
        .expect("should have hash")
        .into();

    // verify version before upgrade
    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let version = *account
        .named_keys()
        .get(VERSION)
        .expect("version uref should exist");

    let original_version = builder
        .query(None, version, &[])
        .expect("version should exist");

    assert_eq!(
        original_version,
        StoredValue::CLValue(CLValue::from_t("1.0.0".to_string()).unwrap()),
        "should be original version"
    );

    // upgrade contract
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_UPGRADER_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    ARG_CONTRACT_PACKAGE => stored_package_hash,
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    // version should change after upgrade
    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let version = *account
        .named_keys()
        .get(VERSION)
        .expect("version key should exist");

    let upgraded_version = builder
        .query(None, version, &[])
        .expect("version should exist");

    assert_eq!(
        upgraded_version,
        StoredValue::CLValue(CLValue::from_t("1.0.1".to_string()).unwrap()),
        "should be original version"
    );
}

#[ignore]
#[test]
fn should_support_extending_functionality() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // store do-nothing-stored
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    ARG_IS_LOCKED => false
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let stored_package_hash = account
        .named_keys()
        .get(HASH_KEY_NAME)
        .expect("should have stored uref")
        .into_package_addr()
        .expect("should have hash");

    let stored_hash = account
        .named_keys()
        .get(PURSE_HOLDER_STORED_CONTRACT_NAME)
        .expect("should have stored uref")
        .into_entity_hash_addr()
        .expect("should have hash")
        .into();

    // call stored contract and persist a known uref before upgrade
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_CALLER_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    HASH_KEY_NAME => stored_hash,
                    ENTRY_POINT_NAME => ENTRY_POINT_ADD,
                    PURSE_NAME => PURSE_1,
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    // verify known uref actually exists prior to upgrade
    let contract = builder
        .get_entity_with_named_keys_by_entity_hash(stored_hash)
        .expect("should have contract");
    assert!(
        contract.named_keys().contains(PURSE_1),
        "purse uref should exist in contract's named_keys before upgrade"
    );

    // upgrade contract
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_UPGRADER_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    ARG_CONTRACT_PACKAGE => stored_package_hash,
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    // verify uref still exists in named_keys after upgrade:
    let contract = builder
        .get_entity_with_named_keys_by_entity_hash(stored_hash)
        .expect("should have contract");

    assert!(
        contract.named_keys().contains(PURSE_1),
        "PURSE_1 uref should still exist in contract's named_keys after upgrade"
    );

    // Get account again after upgrade to refresh named keys
    let account_2 = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    // Get contract again after upgrade

    let stored_hash_2 = account_2
        .named_keys()
        .get(PURSE_HOLDER_STORED_CONTRACT_NAME)
        .expect("should have stored uref")
        .into_entity_hash_addr()
        .expect("should have hash")
        .into();
    assert_ne!(stored_hash, stored_hash_2);

    // call new remove function
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_CALLER_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    HASH_KEY_NAME => stored_hash_2,
                    ENTRY_POINT_NAME => METHOD_REMOVE,
                    PURSE_NAME => PURSE_1,
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    // verify known urefs no longer include removed purse
    let contract = builder
        .get_entity_with_named_keys_by_entity_hash(stored_hash_2)
        .expect("should have contract");

    assert!(
        !contract.named_keys().contains(PURSE_1),
        "PURSE_1 uref should no longer exist in contract's named_keys after remove"
    );
}

#[ignore]
#[test]
fn should_maintain_named_keys_across_upgrade() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // store contract
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    ARG_IS_LOCKED => false
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let stored_hash = account
        .named_keys()
        .get(PURSE_HOLDER_STORED_CONTRACT_NAME)
        .expect("should have stored hash")
        .into_entity_hash_addr()
        .expect("should have hash");

    let stored_package_hash = account
        .named_keys()
        .get(HASH_KEY_NAME)
        .expect("should have stored package hash")
        .into_package_hash()
        .expect("should have hash");

    // add several purse urefs to named_keys
    for index in 0..TOTAL_PURSES {
        let purse_name: &str = &format!("purse_{}", index);

        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_CALLER_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    HASH_KEY_NAME => stored_hash,
                    ENTRY_POINT_NAME => ENTRY_POINT_ADD,
                    PURSE_NAME => purse_name,
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();

        // verify known uref actually exists prior to upgrade
        let contract = builder
            .get_entity_with_named_keys_by_entity_hash(stored_hash.into())
            .expect("should have contract");
        assert!(
            contract.named_keys().contains(purse_name),
            "purse uref should exist in contract's named_keys before upgrade"
        );
    }

    // upgrade contract
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_UPGRADER_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    ARG_CONTRACT_PACKAGE => stored_package_hash,
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    // verify all urefs still exist in named_keys after upgrade
    let contract = builder
        .get_entity_with_named_keys_by_entity_hash(stored_hash.into())
        .expect("should have contract");

    for index in 0..TOTAL_PURSES {
        let purse_name: &str = &format!("purse_{}", index);
        assert!(
            contract.named_keys().contains(purse_name),
            "{} uref should still exist in contract's named_keys after upgrade",
            index
        );
    }
}

#[ignore]
#[test]
fn should_fail_upgrade_for_locked_contract() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // store contract
    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    ARG_IS_LOCKED => true,
                },
            )
            .build()
        };

        builder.exec(exec_request).expect_success().commit();
    }

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let stored_package_hash: PackageHash = account
        .named_keys()
        .get(HASH_KEY_NAME)
        .expect("should have stored package hash")
        .into_package_addr()
        .expect("should have hash")
        .into();

    let contract_package = builder
        .get_package(stored_package_hash)
        .expect("should get package hash");

    // Ensure that our current package is indeed locked.
    assert!(contract_package.is_locked());

    {
        let exec_request = {
            let contract_name = format!("{}.wasm", PURSE_HOLDER_STORED_UPGRADER_CONTRACT_NAME);
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                &contract_name,
                runtime_args! {
                    ARG_CONTRACT_PACKAGE => stored_package_hash,
                },
            )
            .build()
        };

        assert!(builder.exec(exec_request).is_error());
    }
}

#[ignore]
#[test]
fn should_only_upgrade_if_threshold_is_met() {
    const CONTRACT_HASH_NAME: &str = "contract_hash_name";
    const PACKAGE_HASH_KEY_NAME: &str = "contract_package_hash";

    const ENTRYPOINT_ADD_ASSOCIATED_KEY: &str = "add_associated_key";
    const ENTRYPOINT_MANAGE_ACTION_THRESHOLD: &str = "manage_action_threshold";

    const ARG_ENTITY_ACCOUNT_HASH: &str = "entity_account_hash";
    const ARG_KEY_WEIGHT: &str = "key_weight";
    const ARG_NEW_UPGRADE_THRESHOLD: &str = "new_threshold";
    const ARG_CONTRACT_PACKAGE: &str = "contract_package_hash";

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        UPGRADE_THRESHOLD_CONTRACT_NAME,
        runtime_args! {},
    )
    .build();

    builder.exec(install_request).expect_success().commit();

    let entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default addressable entity");

    let upgrade_threshold_contract_hash = entity
        .named_keys()
        .get(CONTRACT_HASH_NAME)
        .expect("must have named key entry for contract hash")
        .into_entity_hash_addr()
        .map(AddressableEntityHash::new)
        .expect("must get contract hash");

    let upgrade_threshold_package_hash = entity
        .named_keys()
        .get(PACKAGE_HASH_KEY_NAME)
        .expect("must have named key entry for package hash")
        .into_package_addr()
        .map(PackageHash::new)
        .expect("must get package hash");

    let upgrade_threshold_contract_entity = builder
        .get_entity_with_named_keys_by_entity_hash(upgrade_threshold_contract_hash)
        .expect("must have upgrade threshold entity");

    let entity = upgrade_threshold_contract_entity.entity();
    let actual_associated_keys = entity.associated_keys();
    let mut expected_associated_keys = AssociatedKeys::new(*DEFAULT_ACCOUNT_ADDR, Weight::new(1));
    assert_eq!(&expected_associated_keys, actual_associated_keys);

    let mut entity_account_hashes =
        vec![AccountHash::new([10u8; 32]), AccountHash::new([11u8; 32])];

    for entity_account_hash in &entity_account_hashes {
        expected_associated_keys
            .add_key(*entity_account_hash, Weight::new(1))
            .expect("must add associated key");

        let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            upgrade_threshold_contract_hash,
            ENTRYPOINT_ADD_ASSOCIATED_KEY,
            runtime_args! {
                ARG_ENTITY_ACCOUNT_HASH => *entity_account_hash,
                ARG_KEY_WEIGHT => 1u8
            },
        )
        .build();

        builder.exec(execute_request).expect_success().commit();
    }

    let update_upgrade_threshold_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        upgrade_threshold_contract_hash,
        ENTRYPOINT_MANAGE_ACTION_THRESHOLD,
        runtime_args! {
            ARG_NEW_UPGRADE_THRESHOLD => 3u8
        },
    )
    .build();

    builder
        .exec(update_upgrade_threshold_request)
        .expect_success()
        .commit();

    let upgrade_threshold_contract_entity = builder
        .get_addressable_entity(upgrade_threshold_contract_hash)
        .expect("must have upgrade threshold entity");

    let updated_associated_keys = upgrade_threshold_contract_entity.associated_keys();
    assert_eq!(&expected_associated_keys, updated_associated_keys);

    let updated_action_threshold = upgrade_threshold_contract_entity.action_thresholds();
    assert_eq!(
        updated_action_threshold.upgrade_management(),
        &Weight::new(3u8)
    );

    let invalid_upgrade_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        UPGRADE_THRESHOLD_UPGRADER,
        runtime_args! {
            ARG_CONTRACT_PACKAGE => upgrade_threshold_package_hash
        },
    )
    .build();

    builder.exec(invalid_upgrade_request).expect_failure();

    builder.assert_error(engine_state::Error::Exec(
        Error::UpgradeAuthorizationFailure,
    ));

    let authorization_keys = {
        entity_account_hashes.push(*DEFAULT_ACCOUNT_ADDR);
        entity_account_hashes
    };

    let valid_upgrade_request = ExecuteRequestBuilder::with_authorization_keys(
        *DEFAULT_ACCOUNT_ADDR,
        UPGRADE_THRESHOLD_UPGRADER,
        runtime_args! {
            ARG_CONTRACT_PACKAGE => upgrade_threshold_package_hash
        },
        &authorization_keys,
    )
    .build();

    builder
        .exec(valid_upgrade_request)
        .expect_success()
        .commit();
}

fn setup_upgrade_threshold_state() -> (LmdbWasmTestBuilder, ProtocolVersion, AccountHash) {
    const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
    const UPGRADE_THRESHOLDS_FIXTURE: &str = "upgrade_thresholds";

    let (mut builder, lmdb_fixture_state, _temp_dir) =
        crate::lmdb_fixture::builder_from_global_state_fixture(UPGRADE_THRESHOLDS_FIXTURE);

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let new_protocol_version =
        ProtocolVersion::from_parts(current_protocol_version.value().major + 1, 0, 0);

    let activation_point = EraId::new(0u64);

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(current_protocol_version)
        .with_new_protocol_version(new_protocol_version)
        .with_activation_point(activation_point)
        .build();

    builder
        .upgrade_using_scratch(
            builder.get_engine_state().config().clone(),
            &mut upgrade_request,
        )
        .expect_upgrade_success();

    let transfer = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => MINIMUM_ACCOUNT_CREATION_BALANCE,
            mint::ARG_ID => Some(42u64),
        },
    )
    .with_protocol_version(new_protocol_version)
    .build();

    builder.exec(transfer).expect_success().commit();

    (builder, new_protocol_version, ACCOUNT_1_ADDR)
}

#[ignore]
#[test]
fn should_migrate_with_correct_upgrade_thresholds() {
    let (mut builder, new_protocol_version, _) = setup_upgrade_threshold_state();

    let default_addressable_entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default entity");

    let contract_hash = default_addressable_entity
        .named_keys()
        .get(PURSE_HOLDER_STORED_CONTRACT_NAME)
        .map(|holder_key| holder_key.into_hash_addr().map(AddressableEntityHash::new))
        .unwrap()
        .expect("must convert to hash");

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}.wasm", PURSE_HOLDER_STORED_CALLER_CONTRACT_NAME),
        runtime_args! {
            ENTRY_POINT_NAME => VERSION,
            HASH_KEY_NAME => contract_hash
        },
    )
    .with_protocol_version(new_protocol_version)
    .build();

    builder.exec(exec_request).expect_success().commit();

    let purse_holder_as_entity = builder
        .get_addressable_entity(contract_hash)
        .expect("must have purse holder entity hash");

    let actual_associated_keys = purse_holder_as_entity.associated_keys();

    let expect_associated_keys = AssociatedKeys::new(*DEFAULT_ACCOUNT_ADDR, Weight::new(1));

    assert_eq!(actual_associated_keys, &expect_associated_keys);
}

#[ignore]
#[test]
fn should_correctly_set_upgrade_threshold_on_entity_upgrade() {
    let (mut builder, new_protocol_version, entity_1) = setup_upgrade_threshold_state();

    let default_addressable_entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default entity");

    let entity_hash = default_addressable_entity
        .named_keys()
        .get(PURSE_HOLDER_STORED_CONTRACT_NAME)
        // We use hash addr as the migration hasn't occurred.
        .map(|holder_key| holder_key.into_hash_addr().map(AddressableEntityHash::new))
        .unwrap()
        .expect("must convert to hash");

    let stored_package_hash = default_addressable_entity
        .named_keys()
        .get(HASH_KEY_NAME)
        .expect("should have stored package hash")
        .into_hash_addr()
        .map(PackageHash::new)
        .expect("should have hash");

    let exec_request = ExecuteRequestBuilder::standard(
        entity_1,
        &format!("{}.wasm", PURSE_HOLDER_STORED_CALLER_CONTRACT_NAME),
        runtime_args! {
            ENTRY_POINT_NAME => VERSION,
            HASH_KEY_NAME => entity_hash
        },
    )
    .with_protocol_version(new_protocol_version)
    .build();

    builder.exec(exec_request).expect_success().commit();

    let purse_holder_as_entity = builder
        .get_addressable_entity(entity_hash)
        .expect("must have purse holder entity hash");

    let actual_associated_keys = purse_holder_as_entity.associated_keys();

    assert!(actual_associated_keys.is_empty());

    let upgrade_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}.wasm", PURSE_HOLDER_STORED_UPGRADER_CONTRACT_NAME),
        runtime_args! {
            ARG_CONTRACT_PACKAGE => stored_package_hash
        },
    )
    .with_protocol_version(new_protocol_version)
    .build();

    builder.exec(upgrade_request).expect_success().commit();

    let new_entity_hash = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have entity")
        .named_keys()
        .get(PURSE_HOLDER_STORED_CONTRACT_NAME)
        .map(|key| key.into_entity_hash_addr().map(AddressableEntityHash::new))
        .unwrap()
        .expect("must get contract hash");

    let updated_purse_entity = builder
        .get_addressable_entity(new_entity_hash)
        .expect("must have purse holder entity hash");

    let actual_associated_keys = updated_purse_entity.associated_keys();

    let expect_associated_keys = AssociatedKeys::new(*DEFAULT_ACCOUNT_ADDR, Weight::new(1));

    assert_eq!(actual_associated_keys, &expect_associated_keys);
}

#[allow(clippy::enum_variant_names)]
enum InvocationType {
    ByContractHash,
    ByContractName,
    ByPackageHash(Option<EntityVersion>),
    ByPackageName(Option<EntityVersion>),
    ByUpgrader,
}

fn call_and_migrate_purse_holder_contract(invocation_type: InvocationType) {
    let (mut builder, new_protocol_version, _) = setup_upgrade_threshold_state();

    let runtime_args = runtime_args! {
        PURSE_NAME_ARG_NAME => PURSE_1
    };

    let default_addressable_entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default entity");

    let entity_hash = default_addressable_entity
        .named_keys()
        .get(PURSE_HOLDER_STORED_CONTRACT_NAME)
        .map(|holder_key| holder_key.into_hash_addr().map(AddressableEntityHash::new))
        .unwrap()
        .expect("must convert to hash");

    let package_hash = default_addressable_entity
        .named_keys()
        .get(HASH_KEY_NAME)
        .expect("must have package named key entry")
        .into_hash_addr()
        .map(PackageHash::new)
        .unwrap();

    let execute_request = match invocation_type {
        InvocationType::ByPackageName(maybe_contract_version) => {
            ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                HASH_KEY_NAME,
                maybe_contract_version,
                ENTRY_POINT_ADD,
                runtime_args,
            )
            .with_protocol_version(new_protocol_version)
            .build()
        }
        InvocationType::ByPackageHash(maybe_contract_version) => {
            ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                package_hash,
                maybe_contract_version,
                ENTRY_POINT_ADD,
                runtime_args,
            )
            .with_protocol_version(new_protocol_version)
            .build()
        }
        InvocationType::ByContractHash => ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            entity_hash,
            ENTRY_POINT_ADD,
            runtime_args,
        )
        .with_protocol_version(new_protocol_version)
        .build(),
        InvocationType::ByContractName => ExecuteRequestBuilder::contract_call_by_name(
            *DEFAULT_ACCOUNT_ADDR,
            PURSE_HOLDER_STORED_CONTRACT_NAME,
            ENTRY_POINT_ADD,
            runtime_args,
        )
        .with_protocol_version(new_protocol_version)
        .build(),
        InvocationType::ByUpgrader => ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            &format!("{}.wasm", PURSE_HOLDER_STORED_UPGRADER_CONTRACT_NAME),
            runtime_args! {
                ARG_CONTRACT_PACKAGE => package_hash
            },
        )
        .with_protocol_version(new_protocol_version)
        .build(),
    };

    builder.exec(execute_request).expect_success().commit();

    let updated_entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default entity");

    let updated_key = updated_entity
        .named_keys()
        .get(PURSE_HOLDER_STORED_CONTRACT_NAME)
        .expect("must have updated entity");

    let updated_hash = if let InvocationType::ByUpgrader = invocation_type {
        updated_key.into_entity_hash()
    } else {
        updated_key.into_hash_addr().map(AddressableEntityHash::new)
    }
    .expect("must get entity hash");

    let updated_purse_entity = builder
        .get_addressable_entity(updated_hash)
        .expect("must have purse holder entity hash");

    let actual_associated_keys = updated_purse_entity.associated_keys();

    let expect_associated_keys = AssociatedKeys::new(*DEFAULT_ACCOUNT_ADDR, Weight::new(1));

    assert_eq!(actual_associated_keys, &expect_associated_keys);
}

#[ignore]
#[test]
fn should_correct_migrate_contract_when_invoked_by_package_name() {
    call_and_migrate_purse_holder_contract(InvocationType::ByPackageName(None))
}

#[ignore]
#[test]
fn should_correctly_migrate_contract_when_invoked_by_name_and_version() {
    call_and_migrate_purse_holder_contract(InvocationType::ByPackageName(Some(INITIAL_VERSION)))
}

#[ignore]
#[test]
fn should_correct_migrate_contract_when_invoked_by_package_hash() {
    call_and_migrate_purse_holder_contract(InvocationType::ByPackageHash(None))
}

#[ignore]
#[test]
fn should_correct_migrate_contract_when_invoked_by_package_hash_and_specific_version() {
    call_and_migrate_purse_holder_contract(InvocationType::ByPackageHash(Some(INITIAL_VERSION)))
}

#[ignore]
#[test]
fn should_correctly_migrate_contract_when_invoked_by_contract_hash() {
    call_and_migrate_purse_holder_contract(InvocationType::ByContractHash)
}

#[ignore]
#[test]
fn should_correctly_migrate_contract_when_invoked_by_contract_name() {
    call_and_migrate_purse_holder_contract(InvocationType::ByContractName)
}

#[ignore]
#[test]
fn should_correctly_migrate_and_upgrade_with_upgrader() {
    call_and_migrate_purse_holder_contract(InvocationType::ByUpgrader)
}
