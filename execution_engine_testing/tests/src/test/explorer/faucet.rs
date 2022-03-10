use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    account::AccountHash, crypto, runtime_args, system::mint, ContractHash, Key, PublicKey,
    RuntimeArgs, SecretKey, U512,
};

// Test constants.
const FAUCET_INSTALLER_SESSION: &str = "faucet_stored.wasm";
const FAUCET_CONTRACT_NAMED_KEY: &str = "faucet";
const INSTALLER_FUND_AMOUNT: u64 = 500_000_000_000_000;
const TWO_HOURS_AS_MILLIS: u64 = 7_200_000;
const FAUCET_ID: u64 = 1337;

// contract args and entry points.
const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";
const ARG_ID: &str = "id";
const ARG_AVAILABLE_AMOUNT: &str = "available_amount";
const ARG_TIME_INTERVAL: &str = "time_interval";
const ARG_DISTRIBUTIONS_PER_INTERVAL: &str = "distributions_per_interval";
const ENTRY_POINT_FAUCET: &str = "call_faucet";
const ENTRY_POINT_SET_VARIABLES: &str = "set_variables";
const ENTRY_POINT_AUTHORIZE_TO: &str = "authorize_to";

// stored contract named keys.
const AVAILABLE_AMOUNT_NAMED_KEY: &str = "available_amount";
const TIME_INTERVAL_NAMED_KEY: &str = "time_interval";
const LAST_DISTRIBUTION_TIME_NAMED_KEY: &str = "last_distribution_time";
const FAUCET_PURSE_NAMED_KEY: &str = "faucet_purse";
const INSTALLER_NAMED_KEY: &str = "installer";
const DISTRIBUTIONS_PER_INTERVAL_NAMED_KEY: &str = "distributions_per_interval";
const REMAINING_REQUESTS_NAMED_KEY: &str = "remaining_requests";
const AUTHORIZED_ACCOUNT_NAMED_KEY: &str = "authorized_account";

#[ignore]
#[test]
fn should_install_faucet_contract() {
    let installer_secret_key =
        SecretKey::ed25519_from_bytes([1; 32]).expect("failed to create secret key");
    let installer_public_key = PublicKey::from(&installer_secret_key);
    let installer_account = AccountHash::from_public_key(&installer_public_key, crypto::blake2b);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => INSTALLER_FUND_AMOUNT,
            mint::ARG_ID => <Option<u64>>::None
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let faucet_fund_amount = U512::from(500_000u64);
    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => FAUCET_ID, ARG_AMOUNT => faucet_fund_amount},
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    let installer_named_keys = builder
        .get_expected_account(installer_account)
        .named_keys()
        .clone();

    assert!(installer_named_keys
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .is_some());

    let faucet_id = format!("faucet_{}", FAUCET_ID);
    assert!(installer_named_keys.get(&faucet_id).is_some());

    let faucet_named_key = installer_named_keys
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .expect("failed to find faucet named key");

    // check installer is set.
    builder
        .query(
            None,
            Key::Hash(
                faucet_named_key
                    .into_hash()
                    .expect("failed to convert key into hash"),
            ),
            &[INSTALLER_NAMED_KEY.to_string()],
        )
        .expect("failed to find installer named key");

    // check time interval
    builder
        .query(
            None,
            Key::Hash(
                faucet_named_key
                    .into_hash()
                    .expect("failed to convert key into hash"),
            ),
            &[TIME_INTERVAL_NAMED_KEY.to_string()],
        )
        .expect("failed to find time interval named key");

    // check last distribution time
    builder
        .query(
            None,
            Key::Hash(
                faucet_named_key
                    .into_hash()
                    .expect("failed to convert key into hash"),
            ),
            &[LAST_DISTRIBUTION_TIME_NAMED_KEY.to_string()],
        )
        .expect("failed to find last distribution named key");

    // check faucet purse
    builder
        .query(
            None,
            Key::Hash(
                faucet_named_key
                    .into_hash()
                    .expect("failed to convert key into hash"),
            ),
            &[FAUCET_PURSE_NAMED_KEY.to_string()],
        )
        .expect("failed to find faucet purse named key");

    // check available amount
    builder
        .query(
            None,
            Key::Hash(
                faucet_named_key
                    .into_hash()
                    .expect("failed to convert key into hash"),
            ),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key");

    // check remaining requests
    builder
        .query(
            None,
            Key::Hash(
                faucet_named_key
                    .into_hash()
                    .expect("failed to convert key into hash"),
            ),
            &[REMAINING_REQUESTS_NAMED_KEY.to_string()],
        )
        .expect("failed to find remaining requests named key");

    builder
        .query(
            None,
            Key::Hash(
                faucet_named_key
                    .into_hash()
                    .expect("failed to convert key into hash"),
            ),
            &[AUTHORIZED_ACCOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find authorized account named key");
}

#[ignore]
#[test]
fn should_allow_installer_to_set_variables() {
    let installer_secret_key =
        SecretKey::ed25519_from_bytes([1; 32]).expect("failed to create secret key");
    let installer_public_key = PublicKey::from(&installer_secret_key);
    let installer_account = AccountHash::from_public_key(&installer_public_key, crypto::blake2b);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => INSTALLER_FUND_AMOUNT,
            mint::ARG_ID => <Option<u64>>::None
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let faucet_fund_amount = U512::from(500_000u64);
    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => FAUCET_ID, ARG_AMOUNT => faucet_fund_amount},
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    let faucet_contract_hash = builder
        .get_expected_account(installer_account)
        .named_keys()
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("failed to find faucet contract");

    // check the balance of the faucet's purse before
    let faucet_contract = builder
        .get_contract(faucet_contract_hash)
        .expect("failed to find faucet contract");

    let faucet_purse = faucet_contract
        .named_keys()
        .get(FAUCET_PURSE_NAMED_KEY)
        .cloned()
        .and_then(Key::into_uref)
        .expect("failed to find faucet purse");

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(faucet_purse_balance, faucet_fund_amount);

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    // the available amount per interval will be zero until the installer calls
    // the set_variable entrypoint to finish setup.
    assert_eq!(available_amount, U512::zero());

    let time_interval = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[TIME_INTERVAL_NAMED_KEY.to_string()],
        )
        .expect("failed to find time interval named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<u64>()
        .expect("failed to convert to u64");

    // defaults to around two hours.
    assert_eq!(time_interval, TWO_HOURS_AS_MILLIS);

    let distributions_per_interval = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[DISTRIBUTIONS_PER_INTERVAL_NAMED_KEY.to_string()],
        )
        .expect("failed to find distributions per interval named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<u64>()
        .expect("failed to convert to u64");

    assert_eq!(distributions_per_interval, 0u64);

    let assigned_time_interval = 10_000u64;
    let assigned_distributions_per_interval = 2u64;

    let installer_set_variable_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(installer_account)
            .with_authorization_keys(&[installer_account])
            .with_stored_session_named_key(
                FAUCET_CONTRACT_NAMED_KEY,
                ENTRY_POINT_SET_VARIABLES,
                runtime_args! {
                    ARG_AVAILABLE_AMOUNT => Some(faucet_fund_amount),
                    ARG_TIME_INTERVAL => Some(assigned_time_interval),
                    ARG_DISTRIBUTIONS_PER_INTERVAL => Some(assigned_distributions_per_interval)
                },
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder
        .exec(installer_set_variable_request)
        .expect_success()
        .commit();

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(available_amount, faucet_fund_amount);

    let time_interval = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[TIME_INTERVAL_NAMED_KEY.to_string()],
        )
        .expect("failed to find time interval named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<u64>()
        .expect("failed to convert to u64");

    assert_eq!(time_interval, assigned_time_interval);

    let distributions_per_interval = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[DISTRIBUTIONS_PER_INTERVAL_NAMED_KEY.to_string()],
        )
        .expect("failed to find distributions per interval named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<u64>()
        .expect("failed to convert to u64");

    assert_eq!(
        distributions_per_interval,
        assigned_distributions_per_interval
    );
}

#[ignore]
#[test]
fn should_fund_new_account() {
    let installer_account = AccountHash::new([1u8; 32]);
    let user_account = AccountHash::new([2u8; 32]);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => INSTALLER_FUND_AMOUNT,
            mint::ARG_ID => <Option<u64>>::None
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let faucet_fund_amount = U512::from(500_000u64);
    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => FAUCET_ID, ARG_AMOUNT => faucet_fund_amount },
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    let faucet_contract_hash = builder
        .get_expected_account(installer_account)
        .named_keys()
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("failed to find faucet contract");

    // check the balance of the faucet's purse before
    let faucet_contract = builder
        .get_contract(faucet_contract_hash)
        .expect("failed to find faucet contract");

    let faucet_purse = faucet_contract
        .named_keys()
        .get(FAUCET_PURSE_NAMED_KEY)
        .cloned()
        .and_then(Key::into_uref)
        .expect("failed to find faucet purse");

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(faucet_purse_balance, faucet_fund_amount);

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    // the available amount per interval will be zero until the installer calls
    // the set_variable entrypoint to finish setup.
    assert_eq!(available_amount, U512::zero());

    let assigned_time_interval = 10_000u64;
    let assigned_distributions_per_interval = 2u64;
    let installer_set_variable_request = ExecuteRequestBuilder::contract_call_by_hash(
        installer_account,
        faucet_contract_hash,
        ENTRY_POINT_SET_VARIABLES,
        runtime_args! {
            ARG_AVAILABLE_AMOUNT => Some(faucet_fund_amount),
            ARG_TIME_INTERVAL => Some(assigned_time_interval),
            ARG_DISTRIBUTIONS_PER_INTERVAL => Some(assigned_distributions_per_interval)
        },
    )
    .build();

    builder
        .exec(installer_set_variable_request)
        .expect_success()
        .commit();

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(available_amount, faucet_fund_amount);

    let remaining_requests = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[REMAINING_REQUESTS_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(
        remaining_requests,
        U512::from(assigned_distributions_per_interval)
    );

    let user_fund_amount = U512::from(100_000);
    let faucet_call_by_installer = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(installer_account)
            .with_authorization_keys(&[installer_account])
            .with_stored_session_named_key(
                FAUCET_CONTRACT_NAMED_KEY,
                ENTRY_POINT_FAUCET,
                runtime_args! {ARG_TARGET => user_account, ARG_AMOUNT => user_fund_amount, ARG_ID => <Option<u64>>::None},
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
            .with_deploy_hash([4; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder
        .exec(faucet_call_by_installer)
        .expect_success()
        .commit();

    let faucet_contract = builder
        .get_contract(faucet_contract_hash)
        .expect("failed to find faucet contract");

    let faucet_purse = faucet_contract
        .named_keys()
        .get(FAUCET_PURSE_NAMED_KEY)
        .cloned()
        .and_then(Key::into_uref)
        .expect("failed to find faucet purse");

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(faucet_purse_balance, faucet_fund_amount - user_fund_amount);

    // check the balance of the user's main purse
    let user_main_purse_balance_after =
        builder.get_purse_balance(builder.get_expected_account(user_account).main_purse());
    assert_eq!(user_main_purse_balance_after, user_fund_amount);
}

#[ignore]
#[test]
fn should_fund_existing_account() {
    let installer_account = AccountHash::new([1u8; 32]);
    let user_account = AccountHash::new([2u8; 32]);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => INSTALLER_FUND_AMOUNT,
            mint::ARG_ID => <Option<u64>>::None
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let faucet_fund_amount = U512::from(200_000_000_000_000u64);
    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => FAUCET_ID, ARG_AMOUNT => faucet_fund_amount},
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    let faucet_contract_hash = builder
        .get_expected_account(installer_account)
        .named_keys()
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("failed to find faucet contract");

    // check the balance of the faucet's purse before
    let faucet_contract = builder
        .get_contract(faucet_contract_hash)
        .expect("failed to find faucet contract");

    let faucet_purse = faucet_contract
        .named_keys()
        .get(FAUCET_PURSE_NAMED_KEY)
        .cloned()
        .and_then(Key::into_uref)
        .expect("failed to find faucet purse");

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(faucet_purse_balance, faucet_fund_amount);

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    // the available amount per interval will be zero until the installer calls
    // the set_variable entrypoint to finish setup.
    assert_eq!(available_amount, U512::zero());

    let assigned_time_interval = 10_000u64;
    let assigned_distributions_per_interval = 2u64;
    let installer_set_variables_request = ExecuteRequestBuilder::contract_call_by_name(
        installer_account,
        FAUCET_CONTRACT_NAMED_KEY,
        ENTRY_POINT_SET_VARIABLES,
        runtime_args! {
            ARG_AVAILABLE_AMOUNT => Some(faucet_fund_amount),
            ARG_TIME_INTERVAL => Some(assigned_time_interval),
            ARG_DISTRIBUTIONS_PER_INTERVAL => Some(assigned_distributions_per_interval)
        },
    )
    .build();

    builder
        .exec(installer_set_variables_request)
        .expect_success()
        .commit();

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(
        available_amount, faucet_fund_amount,
        "available amount must match faucet fund amount"
    );

    let remaining_requests = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[REMAINING_REQUESTS_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(
        remaining_requests,
        U512::from(assigned_distributions_per_interval),
    );

    let user_fund_amount = U512::from(3_000_000_000u64);
    let faucet_call_by_installer = ExecuteRequestBuilder::contract_call_by_name(
        installer_account,
        FAUCET_CONTRACT_NAMED_KEY,
        ENTRY_POINT_FAUCET,
        runtime_args! {
            ARG_TARGET => user_account,
            ARG_AMOUNT => user_fund_amount,
            ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    builder
        .exec(faucet_call_by_installer)
        .expect_success()
        .commit();

    let faucet_contract = builder
        .get_contract(faucet_contract_hash)
        .expect("failed to find faucet contract");

    let faucet_purse = faucet_contract
        .named_keys()
        .get(FAUCET_PURSE_NAMED_KEY)
        .cloned()
        .and_then(Key::into_uref)
        .expect("failed to find faucet purse");

    let half_of_faucet_fund_amount = faucet_fund_amount / 2;
    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(faucet_purse_balance, faucet_fund_amount - user_fund_amount,);

    // check the balance of the user's main purse
    let user_main_purse_balance_after =
        builder.get_purse_balance(builder.get_expected_account(user_account).main_purse());
    assert_eq!(user_main_purse_balance_after, user_fund_amount,);

    let faucet_call_by_user = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(user_account)
            .with_authorization_keys(&[user_account])
            .with_stored_session_hash(
                faucet_contract_hash,
                ENTRY_POINT_FAUCET,
                runtime_args! {ARG_TARGET => user_account, ARG_ID => <Option<u64>>::None},
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => user_fund_amount})
            .with_deploy_hash([4; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder.exec(faucet_call_by_user).expect_success().commit();

    let remaining_requests = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[REMAINING_REQUESTS_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(
        remaining_requests,
        U512::from(assigned_distributions_per_interval - 1),
    );
    let user_main_purse_balance_after =
        builder.get_purse_balance(builder.get_expected_account(user_account).main_purse());

    assert_eq!(user_main_purse_balance_after, half_of_faucet_fund_amount,);

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(
        faucet_purse_balance,
        faucet_fund_amount - half_of_faucet_fund_amount - user_fund_amount,
    );
}

#[ignore]
#[test]
fn should_not_fund_once_exhausted() {
    let installer_account = AccountHash::new([1u8; 32]);
    let user_account = AccountHash::new([2u8; 32]);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => INSTALLER_FUND_AMOUNT,
            mint::ARG_ID => <Option<u64>>::None
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let faucet_fund_amount = U512::from(400_000_000_000_000u64);
    let half_of_faucet_fund_amount = faucet_fund_amount / 2;
    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => FAUCET_ID, ARG_AMOUNT => faucet_fund_amount},
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    let faucet_contract_hash = builder
        .get_expected_account(installer_account)
        .named_keys()
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("failed to find faucet contract");

    // check the balance of the faucet's purse before
    let faucet_contract = builder
        .get_contract(faucet_contract_hash)
        .expect("failed to find faucet contract");

    let faucet_purse = faucet_contract
        .named_keys()
        .get(FAUCET_PURSE_NAMED_KEY)
        .cloned()
        .and_then(Key::into_uref)
        .expect("failed to find faucet purse");

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(faucet_purse_balance, faucet_fund_amount);

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    // the available amount per interval will be zero until the installer calls
    // the set_variable entrypoint to finish setup.
    assert_eq!(available_amount, U512::zero());

    let assigned_time_interval = 10_000u64;
    let assigned_distributions_per_interval = 4u64;
    let assigned_available_amount = half_of_faucet_fund_amount;
    let one_distribution = assigned_available_amount / assigned_distributions_per_interval;
    let installer_set_variable_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(installer_account)
            .with_authorization_keys(&[installer_account])
            .with_stored_session_named_key(
                FAUCET_CONTRACT_NAMED_KEY,
                ENTRY_POINT_SET_VARIABLES,
                runtime_args! {
                    ARG_AVAILABLE_AMOUNT => Some(assigned_available_amount),
                    ARG_TIME_INTERVAL =>  Some(assigned_time_interval),
                    ARG_DISTRIBUTIONS_PER_INTERVAL => Some(assigned_distributions_per_interval)
                },
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder
        .exec(installer_set_variable_request)
        .expect_success()
        .commit();

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(available_amount, half_of_faucet_fund_amount);

    let remaining_requests = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[REMAINING_REQUESTS_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(
        remaining_requests,
        U512::from(assigned_distributions_per_interval)
    );

    let user_fund_amount = U512::from(3_000_000_000u64);
    let num_funds = 4;
    let faucet_call_by_installer = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(installer_account)
            .with_authorization_keys(&[installer_account])
            .with_stored_session_named_key(
                FAUCET_CONTRACT_NAMED_KEY,
                ENTRY_POINT_FAUCET,
                runtime_args! {
                    ARG_TARGET => user_account,
                    ARG_AMOUNT => user_fund_amount * num_funds,
                    ARG_ID => <Option<u64>>::None
                },
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
            .with_deploy_hash([130; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item)
            .with_block_time(1000)
            .build()
    };

    builder
        .exec(faucet_call_by_installer)
        .expect_success()
        .commit();

    for i in 0..num_funds {
        let faucet_call_by_user = {
            let deploy_item = DeployItemBuilder::new()
                .with_address(user_account)
                .with_authorization_keys(&[user_account])
                .with_stored_session_hash(
                    faucet_contract_hash,
                    ENTRY_POINT_FAUCET,
                    runtime_args! {ARG_TARGET => user_account, ARG_ID => <Option<u64>>::None},
                )
                .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => user_fund_amount})
                .with_deploy_hash([i + 10; 32])
                .build();

            ExecuteRequestBuilder::from_deploy_item(deploy_item)
                .with_block_time(1000 + i as u64)
                .build()
        };

        builder.exec(faucet_call_by_user).expect_success().commit();
    }

    let remaining_requests = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[REMAINING_REQUESTS_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(remaining_requests, U512::zero());

    // check the balance of the user's main purse
    let user_main_purse_balance_after =
        builder.get_purse_balance(builder.get_expected_account(user_account).main_purse());
    assert_eq!(
        user_main_purse_balance_after,
        one_distribution * num_funds,
        "users main purse balance must match expected amount after user faucet calls"
    );

    // // call faucet again once it's exhausted.
    let faucet_call_by_user = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(user_account)
            .with_authorization_keys(&[user_account])
            .with_stored_session_hash(
                faucet_contract_hash,
                ENTRY_POINT_FAUCET,
                runtime_args! {ARG_TARGET => user_account, ARG_ID => <Option<u64>>::None},
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => user_fund_amount})
            .with_deploy_hash([20; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item)
            .with_block_time(1010)
            .build()
    };

    builder.exec(faucet_call_by_user).expect_success().commit();

    let user_main_purse_balance_after =
        builder.get_purse_balance(builder.get_expected_account(user_account).main_purse());
    assert_eq!(
        user_main_purse_balance_after,
        one_distribution * num_funds - user_fund_amount,
        "users main purse balance must match expected amount after all user faucet calls"
    );

    // // faucet may resume distributions once block time is > last_distribution_time
    // + time_interval.
    let last_distribution_time = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[LAST_DISTRIBUTION_TIME_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<u64>()
        .expect("failed to convert into U512");

    // we only update this named key after we've passed the threshold.
    assert_eq!(last_distribution_time, 0);

    let faucet_call_by_user = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(user_account)
            .with_authorization_keys(&[user_account])
            .with_stored_session_hash(
                faucet_contract_hash,
                ENTRY_POINT_FAUCET,
                runtime_args! {ARG_TARGET => user_account, ARG_ID => <Option<u64>>::None},
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => user_fund_amount})
            .with_deploy_hash([21; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item)
            .with_block_time(11_011u64)
            .build()
    };

    builder.exec(faucet_call_by_user).expect_success().commit();

    let last_distribution_time = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[LAST_DISTRIBUTION_TIME_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<u64>()
        .expect("failed to convert into U512");

    // we only update this named key after we've passed the threshold.
    assert_eq!(last_distribution_time, 11_011u64);

    let remaining_requests = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[REMAINING_REQUESTS_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(
        remaining_requests,
        U512::from(assigned_distributions_per_interval - 1)
    );

    let user_main_purse_balance_after =
        builder.get_purse_balance(builder.get_expected_account(user_account).main_purse());

    assert_eq!(
        user_main_purse_balance_after,
        one_distribution * (num_funds + 1) - user_fund_amount * 2
    );
}

#[ignore]
#[test]
fn should_allow_installer_to_fund_freely() {
    let installer_account = AccountHash::new([1u8; 32]);
    let user_account = AccountHash::new([2u8; 32]);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => INSTALLER_FUND_AMOUNT,
            mint::ARG_ID => <Option<u64>>::None
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let faucet_fund_amount = U512::from(200_000_000_000u64);
    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => FAUCET_ID, ARG_AMOUNT => faucet_fund_amount},
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    let faucet_contract_hash = builder
        .get_expected_account(installer_account)
        .named_keys()
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("failed to find faucet contract");

    // check the balance of the faucet's purse before
    let faucet_contract = builder
        .get_contract(faucet_contract_hash)
        .expect("failed to find faucet contract");

    let faucet_purse = faucet_contract
        .named_keys()
        .get(FAUCET_PURSE_NAMED_KEY)
        .cloned()
        .and_then(Key::into_uref)
        .expect("failed to find faucet purse");

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(faucet_purse_balance, faucet_fund_amount);

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    // the available amount per interval will be zero until the installer calls
    // the set_variable entrypoint to finish setup.
    assert_eq!(available_amount, U512::zero());

    let assigned_time_interval = 10_000u64;
    let assigned_distributions_per_interval = 2u64;
    let assigned_available_amount = faucet_fund_amount / 2;
    let installer_set_variable_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(installer_account)
            .with_authorization_keys(&[installer_account])
            .with_stored_session_named_key(
                FAUCET_CONTRACT_NAMED_KEY,
                ENTRY_POINT_SET_VARIABLES,
                runtime_args! {
                    ARG_AVAILABLE_AMOUNT => Some(assigned_available_amount),
                    ARG_TIME_INTERVAL => Some(assigned_time_interval),
                    ARG_DISTRIBUTIONS_PER_INTERVAL => Some(assigned_distributions_per_interval)
                },
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder
        .exec(installer_set_variable_request)
        .expect_success()
        .commit();

    let available_amount = builder
        .query(
            None,
            faucet_contract_hash.into(),
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key")
        .as_cl_value()
        .cloned()
        .expect("failed to convert to cl value")
        .into_t::<U512>()
        .expect("failed to convert into U512");

    assert_eq!(available_amount, assigned_available_amount);

    let user_fund_amount = U512::from(3_000_000_000u64);
    // This would only allow other callers to fund twice in this interval,
    // but the installer can fund as many times as they want.
    let num_funds = 3;
    for num in 0..num_funds {
        let faucet_call_by_installer = {
            let deploy_item = DeployItemBuilder::new()
                .with_address(installer_account)
                .with_authorization_keys(&[installer_account])
                .with_stored_session_named_key(
                    FAUCET_CONTRACT_NAMED_KEY,
                    ENTRY_POINT_FAUCET,
                    runtime_args! {ARG_TARGET => user_account, ARG_AMOUNT => user_fund_amount, ARG_ID => <Option<u64>>::None},
                )
                .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
                .with_deploy_hash([num + 4; 32])
                .build();

            ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
        };

        builder
            .exec(faucet_call_by_installer)
            .expect_success()
            .commit();
    }

    let faucet_contract = builder
        .get_contract(faucet_contract_hash)
        .expect("failed to find faucet contract");

    let faucet_purse = faucet_contract
        .named_keys()
        .get(FAUCET_PURSE_NAMED_KEY)
        .cloned()
        .and_then(Key::into_uref)
        .expect("failed to find faucet purse");

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(
        faucet_purse_balance,
        faucet_fund_amount - user_fund_amount * num_funds,
        "faucet purse balance must match expected amount after {} faucet calls",
        num_funds
    );

    // check the balance of the user's main purse
    let user_main_purse_balance_after =
        builder.get_purse_balance(builder.get_expected_account(user_account).main_purse());

    assert_eq!(user_main_purse_balance_after, user_fund_amount * num_funds);
}

#[ignore]
#[test]
fn should_not_fund_if_zero_distributions_per_interval() {
    let installer_account = AccountHash::new([1u8; 32]);
    let user_account = AccountHash::new([2u8; 32]);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => INSTALLER_FUND_AMOUNT,
            mint::ARG_ID => <Option<u64>>::None
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let faucet_fund_amount = U512::from(400_000_000_000_000u64);
    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => FAUCET_ID, ARG_AMOUNT => faucet_fund_amount},
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    let installer_call_faucet_request = ExecuteRequestBuilder::contract_call_by_name(
        installer_account,
        FAUCET_CONTRACT_NAMED_KEY,
        ENTRY_POINT_FAUCET,
        runtime_args! {ARG_TARGET => user_account},
    )
    .build();

    builder
        .exec(installer_call_faucet_request)
        .expect_failure()
        .commit();
}

#[ignore]
#[test]
fn should_allow_funding_by_an_authorized_account() {
    let installer_account = AccountHash::new([1u8; 32]);
    let authorized_account_public_key = {
        let secret_key =
            SecretKey::ed25519_from_bytes([2u8; 32]).expect("failed to construct secret key");
        PublicKey::from(&secret_key)
    };
    let authorized_account = authorized_account_public_key.to_account_hash();
    let user_account = AccountHash::new([3u8; 32]);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => INSTALLER_FUND_AMOUNT,
            mint::ARG_ID => None::<u64>
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let faucet_fund_amount = U512::from(400_000_000_000_000u64);
    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => FAUCET_ID, ARG_AMOUNT => faucet_fund_amount },
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    let assigned_time_interval = 10_000u64;
    let assigned_distributions_per_interval = 2u64;
    let installer_set_variables_request = ExecuteRequestBuilder::contract_call_by_name(
        installer_account,
        FAUCET_CONTRACT_NAMED_KEY,
        ENTRY_POINT_SET_VARIABLES,
        runtime_args! {
            ARG_AVAILABLE_AMOUNT => Some(faucet_fund_amount),
            ARG_TIME_INTERVAL => Some(assigned_time_interval),
            ARG_DISTRIBUTIONS_PER_INTERVAL => Some(assigned_distributions_per_interval)
        },
    )
    .build();

    builder
        .exec(installer_set_variables_request)
        .expect_success()
        .commit();

    let installer_named_keys = builder
        .get_expected_account(installer_account)
        .named_keys()
        .clone();

    let faucet_named_key = installer_named_keys
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .expect("failed to find faucet named key");

    let maybe_authorized_account_public_key = builder
        .query(
            None,
            Key::Hash(
                faucet_named_key
                    .into_hash()
                    .expect("failed to convert key into hash"),
            ),
            &[AUTHORIZED_ACCOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find authorized account named key")
        .as_cl_value()
        .expect("failed to convert into cl value")
        .clone()
        .into_t::<Option<PublicKey>>()
        .expect("failed to convert into optional public key");

    assert_eq!(maybe_authorized_account_public_key, None::<PublicKey>);

    let installer_authorize_caller_request = ExecuteRequestBuilder::contract_call_by_name(
        installer_account,
        FAUCET_CONTRACT_NAMED_KEY,
        ENTRY_POINT_AUTHORIZE_TO,
        runtime_args! {ARG_TARGET => Some(authorized_account_public_key.clone())},
    )
    .build();

    builder
        .exec(installer_authorize_caller_request)
        .expect_success()
        .commit();

    let maybe_authorized_account_public_key = builder
        .query(
            None,
            Key::Hash(
                faucet_named_key
                    .into_hash()
                    .expect("failed to convert key into hash"),
            ),
            &[AUTHORIZED_ACCOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find authorized account named key")
        .as_cl_value()
        .expect("failed to convert into cl value")
        .clone()
        .into_t::<Option<PublicKey>>()
        .expect("failed to convert into optional public key");

    assert_eq!(
        maybe_authorized_account_public_key,
        Some(authorized_account_public_key)
    );

    let authorized_account_fund_amount = U512::from(3_000_000_000u64);
    let faucet_contract_hash = builder
        .get_expected_account(installer_account)
        .named_keys()
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("failed to find faucet contract");

    // the authorized caller needs to have some token in their account to pay for the faucet call.
    let faucet_fund_authorized_account_by_installer_request =
        ExecuteRequestBuilder::contract_call_by_name(
            installer_account,
            FAUCET_CONTRACT_NAMED_KEY,
            ENTRY_POINT_FAUCET,
            runtime_args! {
                ARG_TARGET => authorized_account,
                ARG_AMOUNT => authorized_account_fund_amount,
                ARG_ID => None::<u64>
            },
        )
        .build();

    builder
        .exec(faucet_fund_authorized_account_by_installer_request)
        .expect_success()
        .commit();

    let user_fund_amount = U512::from(3_000_000_000u64);
    let faucet_fund_user_by_authorized_account_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(authorized_account)
            .with_empty_payment_bytes(
                runtime_args! {mint::ARG_AMOUNT => authorized_account_fund_amount},
            )
            .with_stored_session_hash(
                faucet_contract_hash,
                ENTRY_POINT_FAUCET,
                runtime_args! {
                    ARG_TARGET => user_account,
                    ARG_AMOUNT => user_fund_amount,
                    ARG_ID => None::<u64>
                },
            )
            .with_authorization_keys(&[authorized_account])
            .with_deploy_hash([1u8; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder
        .exec(faucet_fund_user_by_authorized_account_request)
        .expect_success()
        .commit();

    let user_main_purse_balance_after =
        builder.get_purse_balance(builder.get_expected_account(user_account).main_purse());
    assert_eq!(user_main_purse_balance_after, user_fund_amount);

    // A user cannot fund themselves if there is an authorized account.

    let faucet_fund_by_user_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_stored_session_hash(
                faucet_contract_hash,
                ENTRY_POINT_FAUCET,
                runtime_args! {
                    ARG_ID => None::<u64>
                },
            )
            .with_address(user_account)
            .with_authorization_keys(&[user_account])
            .with_empty_payment_bytes(runtime_args! {mint::ARG_AMOUNT => user_fund_amount})
            .with_deploy_hash([2u8; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder
        .exec(faucet_fund_by_user_request)
        .expect_failure()
        .commit();

    // TODO: edit wasm test builder to add an easier way to get
    // a handle on errors for making better assertions in tests.

    // let exec_results = builder
    //     .get_last_exec_results()
    //     .expect("failed to get exec results");

    // let exec_result = exec_results
    //     .first()
    //     .expect("an exec result must exist")
    //     .clone();
}

#[ignore]
#[test]
fn faucet_costs() {
    let installer_account = AccountHash::new([1u8; 32]);
    let user_account = AccountHash::new([2u8; 32]);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => INSTALLER_FUND_AMOUNT,
            mint::ARG_ID => <Option<u64>>::None
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let faucet_fund_amount = U512::from(400_000_000_000_000u64);
    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => FAUCET_ID, ARG_AMOUNT => faucet_fund_amount },
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    let _faucet_install_cost = builder.last_exec_gas_cost();
    // println!("faucet install cost: {}", _faucet_install_cost);

    let assigned_time_interval = 10_000u64;
    let assigned_distributions_per_interval = 2u64;
    let installer_set_variable_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(installer_account)
            .with_authorization_keys(&[installer_account])
            .with_stored_session_named_key(
                FAUCET_CONTRACT_NAMED_KEY,
                ENTRY_POINT_SET_VARIABLES,
                runtime_args! {
                    ARG_AVAILABLE_AMOUNT => Some(faucet_fund_amount),
                    ARG_TIME_INTERVAL => Some(assigned_time_interval),
                    ARG_DISTRIBUTIONS_PER_INTERVAL => Some(assigned_distributions_per_interval)
                },
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder
        .exec(installer_set_variable_request)
        .expect_success()
        .commit();

    let _faucet_set_variables_cost = builder.last_exec_gas_cost();
    // println!("faucet set variables cost: {}", _faucet_set_variables_cost);

    let user_fund_amount = U512::from(3_000_000_000u64);
    let faucet_call_by_installer = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(installer_account)
            .with_authorization_keys(&[installer_account])
            .with_stored_session_named_key(
                FAUCET_CONTRACT_NAMED_KEY,
                ENTRY_POINT_FAUCET,
                runtime_args! {ARG_TARGET => user_account, ARG_AMOUNT => user_fund_amount, ARG_ID => <Option<u64>>::None},
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => user_fund_amount})
            .with_deploy_hash([4; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder
        .exec(faucet_call_by_installer)
        .expect_success()
        .commit();

    let _faucet_call_by_installer_cost = builder.last_exec_gas_cost();
    // println!(
    //     "faucet call by installer cost: {}",
    //     _faucet_call_by_installer_cost
    // );

    let faucet_contract_hash = builder
        .get_expected_account(installer_account)
        .named_keys()
        .get(FAUCET_CONTRACT_NAMED_KEY)
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("failed to find faucet contract");

    let faucet_call_by_user_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(user_account)
            .with_authorization_keys(&[user_account])
            .with_stored_session_hash(
                faucet_contract_hash,
                ENTRY_POINT_FAUCET,
                runtime_args! {ARG_TARGET => user_account, ARG_ID => <Option<u64>>::None},
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => user_fund_amount})
            .with_deploy_hash([4; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder
        .exec(faucet_call_by_user_request)
        .expect_success()
        .commit();

    let _faucet_call_by_user_cost = builder.last_exec_gas_cost();
    // println!("faucet call by user cost: {}", _faucet_call_by_user_cost);
}
