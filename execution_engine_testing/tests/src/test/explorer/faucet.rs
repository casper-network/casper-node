use num_rational::Ratio;

use casper_execution_engine::{engine_state, execution::ExecError};

use casper_engine_test_support::{
    ChainspecConfig, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder,
    TransferRequestBuilder, CHAINSPEC_SYMLINK, DEFAULT_PAYMENT, LOCAL_GENESIS_REQUEST,
};
use casper_types::{
    account::AccountHash, addressable_entity::EntityKindTag, runtime_args, ApiError, FeeHandling,
    Key, PricingHandling, PublicKey, RefundHandling, SecretKey, Transfer, U512,
};

// test constants.
use super::{
    faucet_test_helpers::{
        get_faucet_entity_hash, get_faucet_purse, query_stored_value, FaucetDeployHelper,
        FaucetInstallSessionRequestBuilder, FundAccountRequestBuilder,
    },
    ARG_AMOUNT, ARG_AVAILABLE_AMOUNT, ARG_DISTRIBUTIONS_PER_INTERVAL, ARG_ID, ARG_TARGET,
    ARG_TIME_INTERVAL, AUTHORIZED_ACCOUNT_NAMED_KEY, AVAILABLE_AMOUNT_NAMED_KEY,
    DISTRIBUTIONS_PER_INTERVAL_NAMED_KEY, ENTRY_POINT_FAUCET, ENTRY_POINT_SET_VARIABLES,
    FAUCET_CONTRACT_NAMED_KEY, FAUCET_FUND_AMOUNT, FAUCET_ID, FAUCET_INSTALLER_SESSION,
    FAUCET_PURSE_NAMED_KEY, FAUCET_TIME_INTERVAL, INSTALLER_ACCOUNT, INSTALLER_FUND_AMOUNT,
    INSTALLER_NAMED_KEY, LAST_DISTRIBUTION_TIME_NAMED_KEY, REMAINING_REQUESTS_NAMED_KEY,
    TIME_INTERVAL_NAMED_KEY, TWO_HOURS_AS_MILLIS,
};

/// User error variant defined in the faucet contract.
const FAUCET_CALL_BY_USER_WITH_AUTHORIZED_ACCOUNT_SET: u16 = 25;

#[ignore]
#[test]
fn should_install_faucet_contract() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let fund_installer_account_request = FundAccountRequestBuilder::new()
        .with_target_account(INSTALLER_ACCOUNT)
        .with_fund_amount(U512::from(INSTALLER_FUND_AMOUNT))
        .build();

    builder
        .transfer_and_commit(fund_installer_account_request)
        .expect_success();

    builder
        .exec(FaucetInstallSessionRequestBuilder::new().build())
        .expect_success()
        .commit();

    let installer_named_keys = builder
        .get_entity_with_named_keys_by_account_hash(INSTALLER_ACCOUNT)
        .expect("must have entity")
        .named_keys()
        .clone();

    assert!(installer_named_keys
        .get(&format!("{}_{}", FAUCET_CONTRACT_NAMED_KEY, FAUCET_ID))
        .is_some());

    let faucet_purse_id = format!("{}_{}", FAUCET_PURSE_NAMED_KEY, FAUCET_ID);
    assert!(installer_named_keys.get(&faucet_purse_id).is_some());

    let faucet_named_key = installer_named_keys
        .get(&format!("{}_{}", FAUCET_CONTRACT_NAMED_KEY, FAUCET_ID))
        .expect("failed to find faucet named key");

    // check installer is set.
    builder
        .query(None, *faucet_named_key, &[INSTALLER_NAMED_KEY.to_string()])
        .expect("failed to find installer named key");

    // check time interval
    builder
        .query(
            None,
            *faucet_named_key,
            &[TIME_INTERVAL_NAMED_KEY.to_string()],
        )
        .expect("failed to find time interval named key");

    // check last distribution time
    builder
        .query(
            None,
            *faucet_named_key,
            &[LAST_DISTRIBUTION_TIME_NAMED_KEY.to_string()],
        )
        .expect("failed to find last distribution named key");

    // check faucet purse
    builder
        .query(
            None,
            *faucet_named_key,
            &[FAUCET_PURSE_NAMED_KEY.to_string()],
        )
        .expect("failed to find faucet purse named key");

    // check available amount
    builder
        .query(
            None,
            *faucet_named_key,
            &[AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find available amount named key");

    // check remaining requests
    builder
        .query(
            None,
            *faucet_named_key,
            &[REMAINING_REQUESTS_NAMED_KEY.to_string()],
        )
        .expect("failed to find remaining requests named key");

    builder
        .query(
            None,
            *faucet_named_key,
            &[AUTHORIZED_ACCOUNT_NAMED_KEY.to_string()],
        )
        .expect("failed to find authorized account named key");
}

#[ignore]
#[test]
fn should_allow_installer_to_set_variables() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let mut helper = FaucetDeployHelper::new()
        .with_installer_account(INSTALLER_ACCOUNT)
        .with_installer_fund_amount(U512::from(INSTALLER_FUND_AMOUNT))
        .with_faucet_purse_fund_amount(U512::from(FAUCET_FUND_AMOUNT))
        .with_faucet_available_amount(Some(U512::from(FAUCET_FUND_AMOUNT)))
        .with_faucet_distributions_per_interval(Some(2))
        .with_faucet_time_interval(Some(FAUCET_TIME_INTERVAL));

    builder
        .transfer_and_commit(helper.fund_installer_request())
        .expect_success();

    builder
        .exec(helper.faucet_install_request())
        .expect_success()
        .commit();

    let faucet_contract_hash = helper.query_and_set_faucet_contract_hash(&builder);
    let faucet_entity_key =
        Key::addressable_entity_key(EntityKindTag::SmartContract, faucet_contract_hash);

    assert_eq!(
        helper.query_faucet_purse_balance(&builder),
        helper.faucet_purse_fund_amount()
    );

    let available_amount: U512 = query_stored_value(
        &mut builder,
        faucet_entity_key,
        vec![AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
    );

    // the available amount per interval will be zero until the installer calls
    // the set_variable entrypoint to finish setup.
    assert_eq!(available_amount, U512::zero());

    let time_interval: u64 = query_stored_value(
        &mut builder,
        faucet_entity_key,
        vec![TIME_INTERVAL_NAMED_KEY.to_string()],
    );

    // defaults to around two hours.
    assert_eq!(time_interval, TWO_HOURS_AS_MILLIS);

    let distributions_per_interval: u64 = query_stored_value(
        &mut builder,
        faucet_entity_key,
        vec![DISTRIBUTIONS_PER_INTERVAL_NAMED_KEY.to_string()],
    );

    assert_eq!(distributions_per_interval, 0u64);

    builder
        .exec(helper.faucet_config_request())
        .expect_success()
        .commit();

    let available_amount: U512 = query_stored_value(
        &mut builder,
        faucet_entity_key,
        vec![AVAILABLE_AMOUNT_NAMED_KEY.to_string()],
    );

    assert_eq!(available_amount, helper.faucet_purse_fund_amount());

    let time_interval: u64 = query_stored_value(
        &mut builder,
        faucet_entity_key,
        vec![TIME_INTERVAL_NAMED_KEY.to_string()],
    );

    assert_eq!(time_interval, helper.faucet_time_interval().unwrap());

    let distributions_per_interval: u64 = query_stored_value(
        &mut builder,
        faucet_entity_key,
        vec![DISTRIBUTIONS_PER_INTERVAL_NAMED_KEY.to_string()],
    );

    assert_eq!(
        distributions_per_interval,
        helper.faucet_distributions_per_interval().unwrap()
    );
}

#[ignore]
#[test]
fn should_fund_new_account() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let faucet_purse_fund_amount = U512::from(9_000_000_000u64);
    let faucet_distributions_per_interval = 3;

    let mut helper = FaucetDeployHelper::default()
        .with_faucet_purse_fund_amount(faucet_purse_fund_amount)
        .with_faucet_available_amount(Some(faucet_purse_fund_amount))
        .with_faucet_distributions_per_interval(Some(faucet_distributions_per_interval));

    builder
        .transfer_and_commit(helper.fund_installer_request())
        .expect_success();

    builder
        .exec(helper.faucet_install_request())
        .expect_success()
        .commit();

    helper.query_and_set_faucet_contract_hash(&builder);

    builder
        .exec(helper.faucet_config_request())
        .expect_success()
        .commit();

    let new_account = AccountHash::new([7u8; 32]);

    let new_account_fund_amount = U512::from(5_000_000_000u64);
    let fund_new_account_request = helper
        .new_faucet_fund_request_builder()
        .with_installer_account(helper.installer_account())
        .with_arg_target(new_account)
        .with_arg_fund_amount(new_account_fund_amount)
        .build();

    let faucet_purse_uref = helper.query_faucet_purse(&builder);
    let faucet_purse_balance_before = builder.get_purse_balance(faucet_purse_uref);

    builder
        .exec(fund_new_account_request)
        .expect_success()
        .commit();

    let faucet_purse_balance_after = builder.get_purse_balance(faucet_purse_uref);

    assert_eq!(
        faucet_purse_balance_after,
        faucet_purse_balance_before - new_account_fund_amount
    );

    let new_account_actual_purse_balance = builder.get_purse_balance(
        builder
            .get_expected_addressable_entity_by_account_hash(new_account)
            .main_purse(),
    );

    assert_eq!(new_account_actual_purse_balance, new_account_fund_amount);
}

#[ignore]
#[test]
fn should_fund_existing_account() {
    let user_account = AccountHash::new([7u8; 32]);

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let faucet_purse_fund_amount = U512::from(9_000_000_000u64);
    let faucet_distributions_per_interval = 3;

    let mut helper = FaucetDeployHelper::default()
        .with_faucet_purse_fund_amount(faucet_purse_fund_amount)
        .with_faucet_available_amount(Some(faucet_purse_fund_amount))
        .with_faucet_distributions_per_interval(Some(faucet_distributions_per_interval));

    builder
        .transfer_and_commit(helper.fund_installer_request())
        .expect_success();

    let user_account_initial_balance = U512::from(15_000_000_000u64);

    let fund_user_request = FundAccountRequestBuilder::new()
        .with_target_account(user_account)
        .with_fund_amount(user_account_initial_balance)
        .build();

    builder
        .transfer_and_commit(fund_user_request)
        .expect_success();

    builder
        .exec(helper.faucet_install_request())
        .expect_success()
        .commit();

    helper.query_and_set_faucet_contract_hash(&builder);

    builder
        .exec(helper.faucet_config_request())
        .expect_success()
        .commit();

    builder
        .exec(
            helper
                .new_faucet_fund_request_builder()
                .with_user_account(user_account)
                .with_payment_amount(user_account_initial_balance)
                .build(),
        )
        .expect_success()
        .commit();

    let exec_result = builder
        .get_last_exec_result()
        .expect("must have last exec result");
    let transfer = exec_result.transfers().first().expect("must have transfer");

    let one_distribution = Ratio::new(
        faucet_purse_fund_amount,
        faucet_distributions_per_interval.into(),
    )
    .to_integer();
    assert!(
        matches!(transfer, Transfer::V2(v2) if v2.amount == one_distribution),
        "{:?}",
        transfer
    );
}

#[ignore]
#[test]
fn should_allow_installer_to_fund_freely() {
    let installer_account = AccountHash::new([1u8; 32]);
    let user_account = AccountHash::new([2u8; 32]);

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let faucet_fund_amount = U512::from(200_000_000_000u64);
    let half_of_faucet_fund_amount = faucet_fund_amount / 2;
    let assigned_distributions_per_time_interval = 2u64;
    let mut helper = FaucetDeployHelper::new()
        .with_installer_account(installer_account)
        .with_installer_fund_amount(INSTALLER_FUND_AMOUNT.into())
        .with_faucet_purse_fund_amount(faucet_fund_amount)
        .with_faucet_available_amount(Some(half_of_faucet_fund_amount))
        .with_faucet_distributions_per_interval(Some(assigned_distributions_per_time_interval))
        .with_faucet_time_interval(Some(10_000u64));

    builder
        .transfer_and_commit(helper.fund_installer_request())
        .expect_success();

    builder
        .exec(helper.faucet_install_request())
        .expect_success()
        .commit();

    helper.query_and_set_faucet_contract_hash(&builder);

    let faucet_contract_hash = get_faucet_entity_hash(&builder, installer_account);
    let faucet_entity_key =
        Key::addressable_entity_key(EntityKindTag::SmartContract, faucet_contract_hash);
    let faucet_purse = get_faucet_purse(&builder, installer_account);

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(faucet_purse_balance, faucet_fund_amount);

    let available_amount = query_stored_value::<U512>(
        &mut builder,
        faucet_entity_key,
        [AVAILABLE_AMOUNT_NAMED_KEY.to_string()].into(),
    );

    // the available amount per interval should be zero until the installer calls
    // the set_variable entrypoint to finish setup.
    assert_eq!(available_amount, U512::zero());

    builder
        .exec(helper.faucet_config_request())
        .expect_success()
        .commit();

    let available_amount = query_stored_value::<U512>(
        &mut builder,
        faucet_entity_key,
        [AVAILABLE_AMOUNT_NAMED_KEY.to_string()].into(),
    );

    assert_eq!(available_amount, half_of_faucet_fund_amount);

    let user_fund_amount = U512::from(3_000_000_000u64);
    // This would only allow other callers to fund twice in this interval,
    // but the installer can fund as many times as they want.
    let num_funds = 3;

    for _ in 0..num_funds {
        let faucet_call_by_installer = helper
            .new_faucet_fund_request_builder()
            .with_installer_account(helper.installer_account())
            .with_arg_fund_amount(user_fund_amount)
            .with_arg_target(user_account)
            .build();

        builder
            .exec(faucet_call_by_installer)
            .expect_success()
            .commit();
    }

    let faucet_purse_balance = builder.get_purse_balance(faucet_purse);
    assert_eq!(
        faucet_purse_balance,
        faucet_fund_amount - user_fund_amount * num_funds,
        "faucet purse balance must match expected amount after {} faucet calls",
        num_funds
    );

    // check the balance of the user's main purse
    let user_main_purse_balance_after = builder.get_purse_balance(
        builder
            .get_expected_addressable_entity_by_account_hash(user_account)
            .main_purse(),
    );

    assert_eq!(user_main_purse_balance_after, user_fund_amount * num_funds);
}

#[ignore]
#[test]
fn should_not_fund_if_zero_distributions_per_interval() {
    let installer_account = AccountHash::new([1u8; 32]);
    let user_account = AccountHash::new([2u8; 32]);

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // Fund installer account
    let fund_installer_account_request = FundAccountRequestBuilder::new()
        .with_target_account(installer_account)
        .with_fund_amount(INSTALLER_FUND_AMOUNT.into())
        .build();

    builder
        .transfer_and_commit(fund_installer_account_request)
        .expect_success();

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
        &format!("{}_{}", FAUCET_CONTRACT_NAMED_KEY, FAUCET_ID),
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
    let faucet_fund_amount = U512::from(400_000_000_000_000u64);
    let half_of_faucet_fund_amount = faucet_fund_amount / 2;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let mut helper = FaucetDeployHelper::new()
        .with_installer_account(installer_account)
        .with_installer_fund_amount(INSTALLER_FUND_AMOUNT.into())
        .with_faucet_purse_fund_amount(faucet_fund_amount)
        .with_faucet_available_amount(Some(half_of_faucet_fund_amount))
        .with_faucet_distributions_per_interval(Some(2u64))
        .with_faucet_time_interval(Some(10_000u64));

    builder
        .transfer_and_commit(helper.fund_installer_request())
        .expect_success();

    builder
        .exec(helper.faucet_install_request())
        .expect_success()
        .commit();

    helper.query_and_set_faucet_contract_hash(&builder);

    builder
        .exec(helper.faucet_config_request())
        .expect_success()
        .commit();

    let installer_named_keys = builder
        .get_entity_with_named_keys_by_account_hash(installer_account)
        .expect("must have entity")
        .named_keys()
        .clone();

    let faucet_named_key = installer_named_keys
        .get(&format!("{}_{}", FAUCET_CONTRACT_NAMED_KEY, FAUCET_ID))
        .expect("failed to find faucet named key");

    let key = Key::contract_entity_key(faucet_named_key.into_entity_hash().expect(
        "must convert to entity hash\
    ",
    ));

    let maybe_authorized_account_public_key = builder
        .query(None, key, &[AUTHORIZED_ACCOUNT_NAMED_KEY.to_string()])
        .expect("failed to find authorized account named key")
        .as_cl_value()
        .expect("failed to convert into cl value")
        .clone()
        .into_t::<Option<PublicKey>>()
        .expect("failed to convert into optional public key");

    assert_eq!(maybe_authorized_account_public_key, None::<PublicKey>);

    let faucet_authorize_account_request = helper
        .new_faucet_authorize_account_request_builder()
        .with_authorized_user_public_key(Some(authorized_account_public_key.clone()))
        .build();

    builder
        .exec(faucet_authorize_account_request)
        .expect_success()
        .commit();

    let maybe_authorized_account_public_key = builder
        .query(None, key, &[AUTHORIZED_ACCOUNT_NAMED_KEY.to_string()])
        .expect("failed to find authorized account named key")
        .as_cl_value()
        .expect("failed to convert into cl value")
        .clone()
        .into_t::<Option<PublicKey>>()
        .expect("failed to convert into optional public key");

    assert_eq!(
        maybe_authorized_account_public_key,
        Some(authorized_account_public_key.clone())
    );

    let authorized_account_fund_amount = U512::from(10_000_000_000u64);
    let faucet_fund_authorized_account_by_installer_request = helper
        .new_faucet_fund_request_builder()
        .with_arg_fund_amount(authorized_account_fund_amount)
        .with_arg_target(authorized_account_public_key.to_account_hash())
        .build();

    builder
        .exec(faucet_fund_authorized_account_by_installer_request)
        .expect_success()
        .commit();

    let user_fund_amount = U512::from(10_000_000_000u64);
    let faucet_fund_user_by_authorized_account_request = helper
        .new_faucet_fund_request_builder()
        .with_authorized_account(authorized_account)
        .with_arg_fund_amount(user_fund_amount)
        .with_arg_target(user_account)
        .with_payment_amount(user_fund_amount)
        .build();

    builder
        .exec(faucet_fund_user_by_authorized_account_request)
        .expect_success()
        .commit();

    let user_main_purse_balance_after = builder.get_purse_balance(
        builder
            .get_expected_addressable_entity_by_account_hash(user_account)
            .main_purse(),
    );
    assert_eq!(user_main_purse_balance_after, user_fund_amount);

    // A user cannot fund themselves if there is an authorized account.
    let faucet_fund_by_user_request = helper
        .new_faucet_fund_request_builder()
        .with_user_account(user_account)
        .with_payment_amount(user_fund_amount)
        .build();

    builder
        .exec(faucet_fund_by_user_request)
        .expect_failure()
        .commit();

    let exec_result = builder
        .get_last_exec_result()
        .expect("failed to get exec results");

    let error = exec_result.error().unwrap();
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(ExecError::Revert(ApiError::User(
                FAUCET_CALL_BY_USER_WITH_AUTHORIZED_ACCOUNT_SET
            )))
        ),
        "{:?}",
        error,
    );
}

#[ignore]
#[test]
fn faucet_costs() {
    // This test will fail if execution costs vary.  The expected costs should not be updated
    // without understanding why the cost has changed.  If the costs do change, it should be
    // reflected in the "Costs by Entry Point" section of the faucet crate's README.md.
    const EXPECTED_FAUCET_INSTALL_COST: u64 = 91_822_469_530;
    const EXPECTED_FAUCET_SET_VARIABLES_COST: u64 = 111_108_490;
    const EXPECTED_FAUCET_CALL_BY_INSTALLER_COST: u64 = 2_747_581_570;
    const EXPECTED_FAUCET_CALL_BY_USER_COST: u64 = 2_619_470_410;

    let installer_account = AccountHash::new([1u8; 32]);
    let user_account: AccountHash = AccountHash::new([2u8; 32]);

    let chainspec = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must build chainspec configuration");
    let chainspec_config = chainspec
        .with_fee_handling(FeeHandling::NoFee)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_pricing_handling(PricingHandling::Fixed);
    LmdbWasmTestBuilder::new_temporary_with_config(chainspec_config);
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let fund_installer_account_request =
        TransferRequestBuilder::new(INSTALLER_FUND_AMOUNT, installer_account).build();

    builder
        .transfer_and_commit(fund_installer_account_request)
        .expect_success();

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

    let faucet_install_cost = builder.last_exec_gas_cost();

    let assigned_time_interval = 10_000u64;
    let assigned_distributions_per_interval = 2u64;
    let deploy_item = DeployItemBuilder::new()
        .with_address(installer_account)
        .with_authorization_keys(&[installer_account])
        .with_stored_session_named_key(
            &format!("{}_{}", FAUCET_CONTRACT_NAMED_KEY, FAUCET_ID),
            ENTRY_POINT_SET_VARIABLES,
            runtime_args! {
                ARG_AVAILABLE_AMOUNT => Some(faucet_fund_amount),
                ARG_TIME_INTERVAL => Some(assigned_time_interval),
                ARG_DISTRIBUTIONS_PER_INTERVAL => Some(assigned_distributions_per_interval)
            },
        )
        .with_standard_payment(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
        .with_deploy_hash([3; 32])
        .build();

    let installer_set_variable_request =
        ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    builder
        .exec(installer_set_variable_request)
        .expect_success()
        .commit();

    let faucet_set_variables_cost = builder.last_exec_gas_cost();

    let user_fund_amount = U512::from(10_000_000_000u64);

    let deploy_item = DeployItemBuilder::new()
        .with_address(installer_account)
        .with_authorization_keys(&[installer_account])
        .with_stored_session_named_key(
            &format!("{}_{}", FAUCET_CONTRACT_NAMED_KEY, FAUCET_ID),
            ENTRY_POINT_FAUCET,
            runtime_args! {ARG_TARGET => user_account, ARG_AMOUNT => user_fund_amount, ARG_ID => <Option<u64>>::None},
        )
        .with_standard_payment(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
        .with_deploy_hash([4; 32])
        .build();

    let faucet_call_by_installer = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    builder
        .exec(faucet_call_by_installer)
        .expect_success()
        .commit();

    let faucet_call_by_installer_cost = builder.last_exec_gas_cost();

    let faucet_contract_hash = get_faucet_entity_hash(&builder, installer_account);

    let deploy_item = DeployItemBuilder::new()
        .with_address(user_account)
        .with_authorization_keys(&[user_account])
        .with_stored_session_hash(
            faucet_contract_hash,
            ENTRY_POINT_FAUCET,
            runtime_args! {ARG_TARGET => user_account, ARG_ID => <Option<u64>>::None},
        )
        .with_standard_payment(runtime_args! {ARG_AMOUNT => user_fund_amount})
        .with_deploy_hash([4; 32])
        .build();

    let faucet_call_by_user_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    builder
        .exec(faucet_call_by_user_request)
        .expect_success()
        .commit();

    let faucet_call_by_user_cost = builder.last_exec_gas_cost();

    let mut costs_as_expected = true;
    if faucet_install_cost.value().as_u64() != EXPECTED_FAUCET_INSTALL_COST {
        costs_as_expected = false;
        eprintln!(
            "faucet_install_cost wrong: expected: {}, got: {}",
            EXPECTED_FAUCET_INSTALL_COST,
            faucet_install_cost.value().as_u64()
        );
    }

    if faucet_set_variables_cost.value().as_u64() != EXPECTED_FAUCET_SET_VARIABLES_COST {
        costs_as_expected = false;
        eprintln!(
            "faucet_set_variables_cost wrong: expected: {}, got: {}",
            EXPECTED_FAUCET_SET_VARIABLES_COST,
            faucet_set_variables_cost.value().as_u64()
        );
    }

    if faucet_call_by_installer_cost.value().as_u64() != EXPECTED_FAUCET_CALL_BY_INSTALLER_COST {
        costs_as_expected = false;
        eprintln!(
            "faucet_call_by_installer_cost wrong: expected: {}, got: {}",
            EXPECTED_FAUCET_CALL_BY_INSTALLER_COST,
            faucet_call_by_installer_cost.value().as_u64()
        );
    }

    if faucet_call_by_user_cost.value().as_u64() != EXPECTED_FAUCET_CALL_BY_USER_COST {
        costs_as_expected = false;
        eprintln!(
            "faucet_call_by_user_cost wrong: expected: {}, got: {}",
            EXPECTED_FAUCET_CALL_BY_USER_COST,
            faucet_call_by_user_cost.value().as_u64()
        );
    }
    assert!(costs_as_expected);
}
