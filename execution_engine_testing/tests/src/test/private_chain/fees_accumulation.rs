use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_BLOCK_TIME, DEFAULT_PROPOSER_ADDR, DEFAULT_PROTOCOL_VERSION,
    MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::EngineConfigBuilder;
use casper_types::{
    runtime_args,
    system::{handle_payment::ACCUMULATION_PURSE_KEY, mint},
    EntityAddr, EraId, FeeHandling, Key, ProtocolVersion, RuntimeArgs, U512,
};
use once_cell::sync::Lazy;

use crate::{
    lmdb_fixture,
    test::private_chain::{
        self, ACCOUNT_1_ADDR, DEFAULT_ADMIN_ACCOUNT_ADDR, VALIDATOR_1_PUBLIC_KEY,
    },
    wasm_utils,
};

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});

#[ignore]
#[test]
fn default_genesis_config_should_not_have_rewards_purse() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let handle_payment = builder.get_handle_payment_contract_hash();
    let handle_payment_contract =
        builder.get_named_keys(EntityAddr::System(handle_payment.value()));

    assert!(
        handle_payment_contract.contains(ACCUMULATION_PURSE_KEY),
        "Did not find rewards purse in handle payment's named keys {:?}",
        handle_payment_contract
    );
}

#[ignore]
#[test]
fn should_finalize_and_accumulate_rewards_purse() {
    let mut builder = private_chain::setup_genesis_only();

    let handle_payment = builder.get_handle_payment_contract_hash();
    let handle_payment_1 = builder.get_named_keys(EntityAddr::System(handle_payment.value()));

    let rewards_purse_key = handle_payment_1
        .get(ACCUMULATION_PURSE_KEY)
        .expect("should have rewards purse");
    let rewards_purse_uref = rewards_purse_key.into_uref().expect("should be uref");
    assert_eq!(builder.get_purse_balance(rewards_purse_uref), U512::zero());

    let exec_request_1 = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        wasm_utils::do_minimum_bytes(),
        RuntimeArgs::default(),
    )
    .build();

    let exec_request_1_proposer = exec_request_1.proposer.clone();
    let proposer_account_1 = builder
        .get_entity_with_named_keys_by_account_hash(exec_request_1_proposer.to_account_hash())
        .expect("should have proposer account");
    builder.exec(exec_request_1).expect_success().commit();
    assert_eq!(
        builder.get_purse_balance(proposer_account_1.main_purse()),
        U512::zero()
    );

    let handle_payment_2 = builder.get_named_keys(EntityAddr::System(handle_payment.value()));

    assert_eq!(
        handle_payment_1, handle_payment_2,
        "none of the named keys should change before and after execution"
    );

    let exec_request_2 = {
        let transfer_args = runtime_args! {
            mint::ARG_TARGET => *ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        };
        ExecuteRequestBuilder::transfer(*DEFAULT_ADMIN_ACCOUNT_ADDR, transfer_args).build()
    };

    let exec_request_2_proposer = exec_request_2.proposer.clone();
    let proposer_account_2 = builder
        .get_entity_by_account_hash(exec_request_2_proposer.to_account_hash())
        .expect("should have proposer account");

    builder.exec(exec_request_2).expect_success().commit();
    assert_eq!(
        builder.get_purse_balance(proposer_account_2.main_purse()),
        U512::zero()
    );

    let handle_payment_3 = builder.get_named_keys(EntityAddr::System(handle_payment.value()));

    assert_eq!(
        handle_payment_1, handle_payment_2,
        "none of the named keys should change before and after execution"
    );
    assert_eq!(
        handle_payment_2, handle_payment_3,
        "none of the named keys should change before and after execution"
    );
}

#[ignore]
#[test]
fn should_accumulate_deploy_fees() {
    let mut builder = super::private_chain_setup();

    // Check handle payments has rewards purse
    let handle_payment_hash = builder.get_handle_payment_contract_hash();
    let handle_payment_contract =
        builder.get_named_keys(EntityAddr::System(handle_payment_hash.value()));

    let rewards_purse = handle_payment_contract
        .get(ACCUMULATION_PURSE_KEY)
        .unwrap()
        .into_uref()
        .expect("should be uref");

    // At this point rewards purse balance is not zero as the `private_chain_setup` executes bunch
    // of deploys before
    let rewards_balance_before = builder.get_purse_balance(rewards_purse);

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        wasm_utils::do_minimum_bytes(),
        RuntimeArgs::default(),
    )
    .build();

    let exec_request_proposer = exec_request.proposer.clone();

    builder.exec(exec_request).expect_success().commit();

    let handle_payment_after =
        builder.get_named_keys(EntityAddr::System(handle_payment_hash.value()));

    assert_eq!(
        handle_payment_after.get(ACCUMULATION_PURSE_KEY),
        handle_payment_contract.get(ACCUMULATION_PURSE_KEY),
        "keys should not change before and after deploy has been processed",
    );

    let rewards_purse = handle_payment_contract
        .get(ACCUMULATION_PURSE_KEY)
        .unwrap()
        .into_uref()
        .expect("should be uref");
    let rewards_balance_after = builder.get_purse_balance(rewards_purse);
    assert!(
        rewards_balance_after > rewards_balance_before,
        "rewards balance should increase"
    );

    // Ensures default proposer didn't receive any funds
    let proposer_account = builder
        .get_entity_by_account_hash(exec_request_proposer.to_account_hash())
        .expect("should have proposer account");

    assert_eq!(
        builder.get_purse_balance(proposer_account.main_purse()),
        U512::zero()
    );
}

#[ignore]
#[test]
fn should_distribute_accumulated_fees_to_admins() {
    let mut builder = super::private_chain_setup();

    let handle_payment_hash = builder.get_handle_payment_contract_hash();
    let handle_payment = builder.get_named_keys(EntityAddr::System(handle_payment_hash.value()));

    let accumulation_purse = handle_payment
        .get(ACCUMULATION_PURSE_KEY)
        .expect("handle payment should have named key")
        .into_uref()
        .expect("accumulation purse should be an uref");

    let exec_request_1 = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        wasm_utils::do_minimum_bytes(),
        RuntimeArgs::default(),
    )
    .build();

    let accumulated_purse_balance_before_exec = builder.get_purse_balance(accumulation_purse);
    assert!(accumulated_purse_balance_before_exec.is_zero());

    builder.exec(exec_request_1).expect_success().commit();

    // At this point rewards purse balance is not zero as the `private_chain_setup` executes bunch
    // of deploys before
    let accumulated_purse_balance_after_exec = builder.get_purse_balance(accumulation_purse);
    assert!(!accumulated_purse_balance_after_exec.is_zero());

    let admin = builder
        .get_entity_by_account_hash(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have admin account");
    let admin_balance_before = builder.get_purse_balance(admin.main_purse());

    builder
        .distribute(
            None,
            *DEFAULT_PROTOCOL_VERSION,
            &IntoIterator::into_iter([(VALIDATOR_1_PUBLIC_KEY.clone(), U512::from(0))]).collect(),
            1,
            DEFAULT_BLOCK_TIME,
        )
        .expect("should distribute");

    let accumulated_purse_balance_after_distribute = builder.get_purse_balance(accumulation_purse);

    assert!(
        accumulated_purse_balance_after_distribute < accumulated_purse_balance_after_exec,
        "accumulated purse balance should be distributed ({} >= {})",
        accumulated_purse_balance_after_distribute,
        accumulated_purse_balance_after_exec
    );

    let admin_balance_after = builder.get_purse_balance(admin.main_purse());

    assert!(
        admin_balance_after > admin_balance_before,
        "admin balance should grow after distributing accumulated purse"
    );
}

#[ignore]
#[test]
fn should_accumulate_fees_after_upgrade() {
    // let mut builder = super::private_chain_setup();
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_4_5);

    // Ensures default proposer didn't receive any funds
    let proposer_account = builder
        .query(None, Key::Account(*DEFAULT_PROPOSER_ADDR), &[])
        .expect("should have proposer account")
        .into_account()
        .expect("should have legacy Account under the Key::Account variant");

    let proposer_balance_before = builder.get_purse_balance(proposer_account.main_purse());

    // Check handle payments has rewards purse
    let handle_payment_hash = builder.get_handle_payment_contract_hash();

    let handle_payment_contract = builder
        .query(None, Key::Hash(handle_payment_hash.value()), &[])
        .expect("should have handle payment contract")
        .into_contract()
        .expect("should have legacy Contract under the Key::Contract variant");

    assert!(
        handle_payment_contract
            .named_keys()
            .get(ACCUMULATION_PURSE_KEY)
            .is_none(),
        "should not have accumulation purse in a persisted state"
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(EraId::default())
            .with_fee_handling(FeeHandling::Accumulate)
            .build()
    };

    let engine_config = EngineConfigBuilder::default()
        .with_fee_handling(FeeHandling::Accumulate)
        .build();

    builder
        .upgrade_with_upgrade_request_and_config(Some(engine_config), &mut upgrade_request)
        .expect_upgrade_success();
    // Check handle payments has rewards purse
    let handle_payment_hash = builder.get_handle_payment_contract_hash();
    let handle_payment_contract =
        builder.get_named_keys(EntityAddr::System(handle_payment_hash.value()));
    let rewards_purse = handle_payment_contract
        .get(ACCUMULATION_PURSE_KEY)
        .expect("should have accumulation purse")
        .into_uref()
        .expect("should be uref");

    // At this point rewards purse balance is not zero as the `private_chain_setup` executes bunch
    // of deploys before
    let rewards_balance_before = builder.get_purse_balance(rewards_purse);

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        wasm_utils::do_minimum_bytes(),
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let handle_payment_after =
        builder.get_named_keys(EntityAddr::System(handle_payment_hash.value()));

    assert_eq!(
        handle_payment_after.get(ACCUMULATION_PURSE_KEY),
        handle_payment_contract.get(ACCUMULATION_PURSE_KEY),
        "keys should not change before and after deploy has been processed",
    );

    let rewards_purse = handle_payment_contract
        .get(ACCUMULATION_PURSE_KEY)
        .unwrap()
        .into_uref()
        .expect("should be uref");
    let rewards_balance_after = builder.get_purse_balance(rewards_purse);
    assert!(
        rewards_balance_after > rewards_balance_before,
        "rewards balance should increase"
    );

    let proposer_balance_after = builder.get_purse_balance(proposer_account.main_purse());
    assert_eq!(
        proposer_balance_before, proposer_balance_after,
        "proposer should not receive any more funds after switching to accumulation"
    );
}
