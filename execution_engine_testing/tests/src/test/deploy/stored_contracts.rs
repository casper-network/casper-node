use assert_matches::assert_matches;
use casper_engine_test_support::{
    DeployItemBuilder, EntityWithNamedKeys, ExecuteRequestBuilder, LmdbWasmTestBuilder,
    UpgradeRequestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
    DEFAULT_ACCOUNT_KEY, DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution};
use casper_types::{
    account::AccountHash,
    package::{EntityVersion, ENTITY_INITIAL_VERSION},
    runtime_args, EntityVersionKey, EraId, PackageHash, ProtocolVersion, RuntimeArgs, U512,
};

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([42u8; 32]);
const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);
const DO_NOTHING_NAME: &str = "do_nothing";
const DO_NOTHING_CONTRACT_PACKAGE_HASH_NAME: &str = "do_nothing_package_hash";
const DO_NOTHING_CONTRACT_HASH_NAME: &str = "do_nothing_hash";
const INITIAL_VERSION: EntityVersion = ENTITY_INITIAL_VERSION;
const ENTRY_FUNCTION_NAME: &str = "delegate";
const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V1_0_0;
const STORED_PAYMENT_CONTRACT_NAME: &str = "test_payment_stored.wasm";
const STORED_PAYMENT_CONTRACT_HASH_NAME: &str = "test_payment_hash";
const STORED_PAYMENT_CONTRACT_PACKAGE_HASH_NAME: &str = "test_payment_package_hash";
const PAY_ENTRYPOINT: &str = "pay";
const TRANSFER_PURSE_TO_ACCOUNT_CONTRACT_NAME: &str = "transfer_purse_to_account";

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

/// Prepares a upgrade request with pre-loaded deploy code, and new protocol version.
fn make_upgrade_request(new_protocol_version: ProtocolVersion) -> UpgradeRequestBuilder {
    UpgradeRequestBuilder::new()
        .with_current_protocol_version(PROTOCOL_VERSION)
        .with_new_protocol_version(new_protocol_version)
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
}

fn install_custom_payment(
    builder: &mut LmdbWasmTestBuilder,
) -> (EntityWithNamedKeys, PackageHash, U512) {
    // store payment contract
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORED_PAYMENT_CONTRACT_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).commit();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    // check account named keys
    let package_hash = default_account
        .named_keys()
        .get(STORED_PAYMENT_CONTRACT_PACKAGE_HASH_NAME)
        .expect("key should exist")
        .into_package_hash()
        .expect("should be a hash");

    let exec_cost = builder.last_exec_result().cost().value();

    (default_account, package_hash, exec_cost)
}

#[ignore]
#[test]
fn should_exec_non_stored_code() {
    // using the new execute logic, passing code for both payment and session
    // should work exactly as it did with the original exec logic

    let account_1_account_hash = ACCOUNT_1_ADDR;
    let payment_purse_amount = *DEFAULT_PAYMENT;
    let transferred_amount = 1;

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(
                format!("{}.wasm", TRANSFER_PURSE_TO_ACCOUNT_CONTRACT_NAME),
                runtime_args! {
                    ARG_TARGET => account_1_account_hash,
                    ARG_AMOUNT => U512::from(transferred_amount)
                },
            )
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_purse_amount,
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([1; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request).expect_success().commit();

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get genesis account");
    let modified_balance: U512 = builder.get_purse_balance(default_account.main_purse());

    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);

    assert_ne!(
        modified_balance, initial_balance,
        "balance should be less than initial balance"
    );

    let tally = transaction_fee + U512::from(transferred_amount) + modified_balance;

    assert_eq!(
        initial_balance, tally,
        "no net resources should be gained or lost post-distribution"
    );
}

#[ignore]
#[test]
fn should_fail_if_calling_non_existent_entry_point() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // first, store payment contract with entry point named "pay"
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORED_PAYMENT_CONTRACT_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).commit();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract associated with default account");
    let stored_payment_contract_hash = default_account
        .named_keys()
        .get(STORED_PAYMENT_CONTRACT_HASH_NAME)
        .expect("should have standard_payment named key")
        .into_entity_hash_addr()
        .expect("standard_payment should be an uref");

    // next make another deploy that attempts to use the stored payment logic
    // but passing the name for an entry point that does not exist.
    let exec_request_stored_payment = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(format!("{}.wasm", DO_NOTHING_NAME), RuntimeArgs::default())
            .with_stored_payment_hash(
                stored_payment_contract_hash.into(),
                "electric-boogaloo",
                runtime_args! { ARG_AMOUNT => payment_purse_amount },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([1; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_stored_payment).commit();

    assert!(
        builder.is_error(),
        "calling a non-existent entry point should not work"
    );

    let expected_error = Error::Exec(execution::Error::NoSuchMethod(
        "electric-boogaloo".to_string(),
    ));

    builder.assert_error(expected_error);
}

#[ignore]
#[test]
fn should_exec_stored_code_by_hash() {
    let default_payment = *DEFAULT_PAYMENT;

    // genesis
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // store payment
    let (sending_account, custom_payment_package_hash, _) = install_custom_payment(&mut builder);

    // verify stored contract functions as expected by checking all the maths
    let sending_account_balance: U512 = builder.get_purse_balance(sending_account.main_purse());

    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);

    assert!(
        sending_account_balance < initial_balance,
        "balance should be less than initial balance"
    );

    let transferred_amount = U512::one();

    // next make another deploy that USES stored payment logic

    {
        let transfer_using_stored_payment = {
            let deploy = DeployItemBuilder::new()
                .with_address(*DEFAULT_ACCOUNT_ADDR)
                .with_stored_versioned_payment_contract_by_hash(
                    custom_payment_package_hash.value(),
                    Some(ENTITY_INITIAL_VERSION),
                    PAY_ENTRYPOINT,
                    runtime_args! {
                        ARG_AMOUNT => default_payment,
                    },
                )
                .with_session_code(
                    format!("{}.wasm", TRANSFER_PURSE_TO_ACCOUNT_CONTRACT_NAME),
                    runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => transferred_amount },
                )
                .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
                .with_deploy_hash([2; 32])
                .build();

            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        builder.exec(transfer_using_stored_payment).expect_failure();
    }

    let error = builder.get_error().unwrap();

    assert_matches!(error, Error::Exec(execution::Error::ForgedReference(_)))
}

#[ignore]
#[test]
fn should_not_transfer_above_balance_using_stored_payment_code_by_hash() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    // genesis
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // store payment
    let (default_account, hash, _) = install_custom_payment(&mut builder);
    let starting_balance = builder.get_purse_balance(default_account.main_purse());

    let transferred_amount = starting_balance - *DEFAULT_PAYMENT + U512::one();

    let exec_request_stored_payment = {
        let account_1_account_hash = ACCOUNT_1_ADDR;
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(
                format!("{}.wasm", TRANSFER_PURSE_TO_ACCOUNT_CONTRACT_NAME),
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => transferred_amount },
            )
            .with_stored_versioned_payment_contract_by_hash(
                hash.value(),
                Some(ENTITY_INITIAL_VERSION),
                PAY_ENTRYPOINT,
                runtime_args! {
                    ARG_AMOUNT => payment_purse_amount,
                },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder
        .exec(exec_request_stored_payment)
        .expect_failure()
        .commit();

    let error = builder.get_error().unwrap();

    assert_matches!(error, Error::Exec(execution::Error::ForgedReference(_)))
}

#[ignore]
#[test]
fn should_empty_account_using_stored_payment_code_by_hash() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    // genesis
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // store payment

    let (default_account, hash, _) = install_custom_payment(&mut builder);
    let starting_balance = builder.get_purse_balance(default_account.main_purse());

    // verify stored contract functions as expected by checking all the maths

    let transferred_amount = starting_balance - *DEFAULT_PAYMENT;

    {
        let exec_request_stored_payment = {
            let account_1_account_hash = ACCOUNT_1_ADDR;
            let deploy = DeployItemBuilder::new()
                .with_address(*DEFAULT_ACCOUNT_ADDR)
                .with_session_code(
                    format!("{}.wasm", TRANSFER_PURSE_TO_ACCOUNT_CONTRACT_NAME),
                    runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => transferred_amount },
                )
                .with_stored_versioned_payment_contract_by_hash(
                    hash.value(),
                    Some(ENTITY_INITIAL_VERSION),
                    PAY_ENTRYPOINT,
                    runtime_args! {
                        ARG_AMOUNT => payment_purse_amount,
                    },
                )
                .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
                .with_deploy_hash([2; 32])
                .build();

            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        builder.exec(exec_request_stored_payment).expect_failure();
    }

    let error = builder.get_error().expect("must have error");

    assert_matches!(error, Error::Exec(execution::Error::ForgedReference(_)))
}

#[ignore]
#[test]
fn should_exec_stored_code_by_named_hash() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    // genesis
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    install_custom_payment(&mut builder);

    // verify stored contract functions as expected by checking all the maths

    let transferred_amount = 1;

    {
        let exec_request_stored_payment = {
            let account_1_account_hash = ACCOUNT_1_ADDR;
            let deploy = DeployItemBuilder::new()
                .with_address(*DEFAULT_ACCOUNT_ADDR)
                .with_session_code(
                    format!("{}.wasm", TRANSFER_PURSE_TO_ACCOUNT_CONTRACT_NAME),
                    runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => U512::from(transferred_amount) },
                )
                .with_stored_versioned_payment_contract_by_name(
                    STORED_PAYMENT_CONTRACT_PACKAGE_HASH_NAME,
                    Some(ENTITY_INITIAL_VERSION),
                    PAY_ENTRYPOINT,
                    runtime_args! {
                        ARG_AMOUNT => payment_purse_amount,
                    },
                )
                .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
                .with_deploy_hash([2; 32])
                .build();

            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        builder.exec(exec_request_stored_payment).expect_failure();

        let error = builder.get_error().unwrap();

        assert_matches!(error, Error::Exec(execution::Error::ForgedReference(_)))
    }
}

#[ignore]
#[test]
fn should_fail_payment_stored_at_named_key_with_incompatible_major_version() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    // first, store payment contract
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORED_PAYMENT_CONTRACT_NAME,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request).commit();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

    assert!(
        default_account
            .named_keys()
            .contains(STORED_PAYMENT_CONTRACT_HASH_NAME),
        "standard_payment should be present"
    );

    //
    // upgrade with new wasm costs with modified mint for given version to avoid missing wasm costs
    // table that's queried early
    //
    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major + 1, sem_ver.minor, sem_ver.patch);

    let mut upgrade_request = make_upgrade_request(new_protocol_version).build();

    builder
        .upgrade_with_upgrade_request_and_config(None, &mut upgrade_request)
        .expect_upgrade_success();

    // next make another deploy that USES stored payment logic
    let exec_request_stored_payment = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(format!("{}.wasm", DO_NOTHING_NAME), RuntimeArgs::default())
            .with_stored_payment_named_key(
                STORED_PAYMENT_CONTRACT_HASH_NAME,
                PAY_ENTRYPOINT,
                runtime_args! {
                    ARG_AMOUNT => payment_purse_amount,
                },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(new_protocol_version)
            .build()
    };

    builder.exec(exec_request_stored_payment).commit();

    assert!(
        builder.is_error(),
        "calling a payment module with increased major protocol version should be error"
    );

    let expected_error = Error::Exec(execution::Error::IncompatibleProtocolMajorVersion {
        expected: 2,
        actual: 1,
    });

    builder.assert_error(expected_error);
}

#[ignore]
#[test]
fn should_fail_payment_stored_at_hash_with_incompatible_major_version() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    // first, store payment contract
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORED_PAYMENT_CONTRACT_NAME,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request).commit();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract associated with default account");
    let stored_payment_contract_hash = default_account
        .named_keys()
        .get(STORED_PAYMENT_CONTRACT_HASH_NAME)
        .expect("should have standard_payment named key")
        .into_entity_hash_addr()
        .expect("standard_payment should be an uref");

    //
    // upgrade with new wasm costs with modified mint for given version to avoid missing wasm costs
    // table that's queried early
    //
    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major + 1, sem_ver.minor, sem_ver.patch);

    let mut upgrade_request = make_upgrade_request(new_protocol_version).build();

    builder
        .upgrade_with_upgrade_request_and_config(None, &mut upgrade_request)
        .expect_upgrade_success();

    // next make another deploy that USES stored payment logic
    let exec_request_stored_payment = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(format!("{}.wasm", DO_NOTHING_NAME), RuntimeArgs::default())
            .with_stored_payment_hash(
                stored_payment_contract_hash.into(),
                PAY_ENTRYPOINT,
                runtime_args! { ARG_AMOUNT => payment_purse_amount },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(new_protocol_version)
            .build()
    };

    builder.exec(exec_request_stored_payment).commit();

    assert!(
        builder.is_error(),
        "calling a payment module with increased major protocol version should be error"
    );

    let expected_error = Error::Exec(execution::Error::IncompatibleProtocolMajorVersion {
        expected: 2,
        actual: 1,
    });

    builder.assert_error(expected_error);
}

#[ignore]
#[test]
fn should_fail_session_stored_at_named_key_with_incompatible_major_version() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // first, store payment contract for v1.0.0
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}_stored.wasm", DO_NOTHING_NAME),
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).commit();

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORED_PAYMENT_CONTRACT_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).commit();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract associated with default account");
    assert!(
        default_account
            .named_keys()
            .contains(DO_NOTHING_CONTRACT_HASH_NAME),
        "do_nothing should be present in named keys"
    );

    let stored_payment_contract_hash = default_account
        .named_keys()
        .get(STORED_PAYMENT_CONTRACT_HASH_NAME)
        .expect("should have standard_payment named key")
        .into_entity_hash_addr()
        .expect("standard_payment should be an uref");
    //
    // upgrade with new wasm costs with modified mint for given version
    //
    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major + 1, sem_ver.minor, sem_ver.patch);

    let mut upgrade_request = make_upgrade_request(new_protocol_version).build();

    builder
        .upgrade_with_upgrade_request_and_config(None, &mut upgrade_request)
        .expect_upgrade_success();

    // Call stored session code

    let exec_request_stored_payment = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_named_key(
                DO_NOTHING_CONTRACT_HASH_NAME,
                ENTRY_FUNCTION_NAME,
                RuntimeArgs::new(),
            )
            .with_stored_payment_hash(
                stored_payment_contract_hash.into(),
                PAY_ENTRYPOINT,
                runtime_args! { ARG_AMOUNT => payment_purse_amount },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(new_protocol_version)
            .build()
    };

    builder.exec(exec_request_stored_payment).commit();

    assert!(
        builder.is_error(),
        "calling a session module with increased major protocol version should be error",
    );
    let error = builder.get_error().expect("must have error");
    assert!(matches!(
        error,
        Error::Exec(execution::Error::IncompatibleProtocolMajorVersion {
            expected: 2,
            actual: 1
        })
    ))
}

#[ignore]
#[test]
fn should_fail_session_stored_at_named_key_with_missing_new_major_version() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    // first, store payment contract for v1.0.0
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}_stored.wasm", DO_NOTHING_NAME),
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).commit();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");
    assert!(
        default_account
            .named_keys()
            .contains(DO_NOTHING_CONTRACT_HASH_NAME),
        "do_nothing should be present in named keys"
    );

    //
    // upgrade with new wasm costs with modified mint for given version
    //
    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major + 1, sem_ver.minor, sem_ver.patch);

    let mut upgrade_request = make_upgrade_request(new_protocol_version).build();

    builder
        .upgrade_with_upgrade_request_and_config(None, &mut upgrade_request)
        .expect_upgrade_success();

    // Call stored session code

    let exec_request_stored_payment = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                DO_NOTHING_CONTRACT_PACKAGE_HASH_NAME,
                Some(INITIAL_VERSION),
                ENTRY_FUNCTION_NAME,
                RuntimeArgs::new(),
            )
            .with_payment_code(
                STORED_PAYMENT_CONTRACT_NAME,
                runtime_args! {
                    ARG_AMOUNT => payment_purse_amount,
                },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(new_protocol_version)
            .build()
    };

    builder.exec(exec_request_stored_payment).commit();

    assert!(
        builder.is_error(),
        "calling a session module with increased major protocol version should be error",
    );

    let entity_version_key = EntityVersionKey::new(2, 1);

    let expected_error = Error::Exec(execution::Error::InvalidEntityVersion(entity_version_key));

    builder.assert_error(expected_error);
}

#[ignore]
#[test]
fn should_fail_session_stored_at_hash_with_incompatible_major_version() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // first, store payment contract for v1.0.0
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}_stored.wasm", DO_NOTHING_NAME),
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).commit();

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORED_PAYMENT_CONTRACT_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).commit();

    //
    // upgrade with new wasm costs with modified mint for given version
    //
    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major + 1, sem_ver.minor, sem_ver.patch);

    let mut upgrade_request = make_upgrade_request(new_protocol_version).build();

    builder
        .upgrade_with_upgrade_request_and_config(None, &mut upgrade_request)
        .expect_upgrade_success();

    // Call stored session code

    // query both stored contracts by their named keys
    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");
    let test_payment_stored_hash = default_account
        .named_keys()
        .get(STORED_PAYMENT_CONTRACT_HASH_NAME)
        .expect("standard_payment should be present in named keys")
        .into_entity_hash_addr()
        .expect("standard_payment named key should be hash");

    let exec_request_stored_payment = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_named_key(
                DO_NOTHING_CONTRACT_HASH_NAME,
                ENTRY_FUNCTION_NAME,
                RuntimeArgs::new(),
            )
            .with_stored_payment_hash(
                test_payment_stored_hash.into(),
                PAY_ENTRYPOINT,
                runtime_args! { ARG_AMOUNT => payment_purse_amount },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(new_protocol_version)
            .build()
    };

    builder.exec(exec_request_stored_payment).commit();

    assert!(
        builder.is_error(),
        "calling a session module with increased major protocol version should be error",
    );
    let error = builder.get_error().expect("must have error");
    assert!(
        matches!(
            error,
            Error::Exec(execution::Error::IncompatibleProtocolMajorVersion {
                expected: 2,
                actual: 1
            }),
        ),
        "Error does not match: {:?}",
        error
    )
}

#[ignore]
#[test]
fn should_execute_stored_payment_and_session_code_with_new_major_version() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    //
    // upgrade with new wasm costs with modified mint for given version
    //
    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major + 1, sem_ver.minor, sem_ver.patch);

    let mut upgrade_request = make_upgrade_request(new_protocol_version).build();

    builder
        .upgrade_with_upgrade_request_and_config(None, &mut upgrade_request)
        .expect_upgrade_success();

    // first, store payment contract for v2.0.0

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORED_PAYMENT_CONTRACT_NAME,
        RuntimeArgs::default(),
    )
    .with_protocol_version(new_protocol_version)
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}_stored.wasm", DO_NOTHING_NAME),
        RuntimeArgs::default(),
    )
    .with_protocol_version(new_protocol_version)
    .build();

    // store both contracts
    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_2).expect_success().commit();

    // query both stored contracts by their named keys
    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");
    let test_payment_stored_hash = default_account
        .named_keys()
        .get(STORED_PAYMENT_CONTRACT_HASH_NAME)
        .expect("standard_payment should be present in named keys")
        .into_entity_hash_addr()
        .expect("standard_payment named key should be hash");

    let exec_request_stored_payment = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                DO_NOTHING_CONTRACT_PACKAGE_HASH_NAME,
                Some(INITIAL_VERSION),
                ENTRY_FUNCTION_NAME,
                RuntimeArgs::new(),
            )
            .with_stored_payment_hash(
                test_payment_stored_hash.into(),
                PAY_ENTRYPOINT,
                runtime_args! { ARG_AMOUNT => payment_purse_amount },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(new_protocol_version)
            .build()
    };

    builder
        .clear_results()
        .exec(exec_request_stored_payment)
        .expect_failure();

    let error = builder.get_error().unwrap();

    assert_matches!(error, Error::Exec(execution::Error::ForgedReference(_)))
}
