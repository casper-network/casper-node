use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, StepRequestBuilder, UpgradeRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PROPOSER_ADDR, DEFAULT_PROTOCOL_VERSION,
    MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST, TIMESTAMP_MILLIS_INCREMENT,
};
use casper_execution_engine::core::engine_state::{
    engine_config::FeeHandling, EngineConfigBuilder, RewardItem,
};
use casper_types::{
    runtime_args,
    system::{handle_payment::ACCUMULATION_PURSE_KEY, mint},
    EraId, ProtocolVersion, RuntimeArgs, U512,
};
use once_cell::sync::Lazy;

use crate::{
    lmdb_fixture,
    test::private_chain::{
        self, ACCOUNT_1_ADDR, DEFAULT_ADMIN_ACCOUNT_ADDR, VALIDATOR_1_PUBLIC_KEY,
    },
    wasm_utils,
};

const VALIDATOR_1_REWARD_FACTOR: u64 = 0;

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
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let handle_payment = builder.get_handle_payment_contract_hash();
    let handle_payment_contract = builder
        .get_contract(handle_payment)
        .expect("should have handle payment contract");

    assert!(
        handle_payment_contract
            .named_keys()
            .contains_key(ACCUMULATION_PURSE_KEY),
        "Did not find rewards purse in handle payment's named keys {:?}",
        handle_payment_contract.named_keys()
    );
}

#[ignore]
#[test]
fn should_finalize_and_accumulate_rewards_purse() {
    let mut builder = private_chain::setup_genesis_only();

    let handle_payment = builder.get_handle_payment_contract_hash();
    let handle_payment_1 = builder
        .get_contract(handle_payment)
        .expect("should have handle payment contract");

    let rewards_purse_key = handle_payment_1
        .named_keys()
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
        .get_account(exec_request_1_proposer.to_account_hash())
        .expect("should have proposer account");
    builder.exec(exec_request_1).expect_success().commit();
    assert_eq!(
        builder.get_purse_balance(proposer_account_1.main_purse()),
        U512::zero()
    );

    let handle_payment_2 = builder
        .get_contract(handle_payment)
        .expect("should have handle payment contract");

    assert_eq!(
        handle_payment_1.named_keys(),
        handle_payment_2.named_keys(),
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
        .get_account(exec_request_2_proposer.to_account_hash())
        .expect("should have proposer account");

    builder.exec(exec_request_2).expect_success().commit();
    assert_eq!(
        builder.get_purse_balance(proposer_account_2.main_purse()),
        U512::zero()
    );

    let handle_payment_3 = builder
        .get_contract(handle_payment)
        .expect("should have handle payment contract");

    assert_eq!(
        handle_payment_1.named_keys(),
        handle_payment_2.named_keys(),
        "none of the named keys should change before and after execution"
    );
    assert_eq!(
        handle_payment_2.named_keys(),
        handle_payment_3.named_keys(),
        "none of the named keys should change before and after execution"
    );
}

#[ignore]
#[test]
fn should_accumulate_deploy_fees() {
    let mut builder = super::private_chain_setup();

    // Check handle payments has rewards purse
    let handle_payment_hash = builder.get_handle_payment_contract_hash();
    let handle_payment_contract = builder
        .get_contract(handle_payment_hash)
        .expect("should have handle payment contract");

    let rewards_purse = handle_payment_contract.named_keys()[ACCUMULATION_PURSE_KEY]
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

    let handle_payment_after = builder
        .get_contract(handle_payment_hash)
        .expect("should have handle payment contract");

    assert_eq!(
        handle_payment_after
            .named_keys()
            .get(ACCUMULATION_PURSE_KEY),
        handle_payment_contract
            .named_keys()
            .get(ACCUMULATION_PURSE_KEY),
        "keys should not change before and after deploy has been processed",
    );

    let rewards_purse = handle_payment_contract.named_keys()[ACCUMULATION_PURSE_KEY]
        .into_uref()
        .expect("should be uref");
    let rewards_balance_after = builder.get_purse_balance(rewards_purse);
    assert!(
        rewards_balance_after > rewards_balance_before,
        "rewards balance should increase"
    );

    // Ensures default proposer didn't receive any funds
    let proposer_account = builder
        .get_account(exec_request_proposer.to_account_hash())
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
    let handle_payment = builder
        .get_contract(handle_payment_hash)
        .expect("should have handle payment contract");
    let accumulation_purse = handle_payment.named_keys()[ACCUMULATION_PURSE_KEY]
        .as_uref()
        .cloned()
        .unwrap();

    let exec_request_1 = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        wasm_utils::do_minimum_bytes(),
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    // At this point rewards purse balance is not zero as the `private_chain_setup` executes bunch
    // of deploys before
    let accumulated_purse_balance_before = builder.get_purse_balance(accumulation_purse);
    assert!(!accumulated_purse_balance_before.is_zero());

    let admin = builder
        .get_account(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have admin account");
    let admin_balance_before = builder.get_purse_balance(admin.main_purse());

    let mut timestamp_millis = 0;
    for _ in 0..3 {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(*DEFAULT_PROTOCOL_VERSION)
            .with_next_era_id(builder.get_era().successor())
            .with_era_end_timestamp_millis(timestamp_millis)
            .with_run_auction(true)
            .with_reward_item(RewardItem::new(
                VALIDATOR_1_PUBLIC_KEY.clone(),
                VALIDATOR_1_REWARD_FACTOR,
            ))
            .build();
        builder.step(step_request).expect("should execute step");
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let last_trusted_era = builder.get_era();

    let step_request = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(*DEFAULT_PROTOCOL_VERSION)
        .with_reward_item(RewardItem::new(
            VALIDATOR_1_PUBLIC_KEY.clone(),
            VALIDATOR_1_REWARD_FACTOR,
        ))
        .with_next_era_id(last_trusted_era.successor())
        .with_era_end_timestamp_millis(timestamp_millis)
        .with_run_auction(true)
        .build();

    builder.step(step_request).expect("should execute step");

    let accumulated_purse_balance_after = builder.get_purse_balance(accumulation_purse);

    assert!(
        accumulated_purse_balance_after < accumulated_purse_balance_before,
        "accumulated purse balance should be distributed"
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
        .get_account(*DEFAULT_PROPOSER_ADDR)
        .expect("should have proposer account");

    let proposer_balance_before = builder.get_purse_balance(proposer_account.main_purse());

    // Check handle payments has rewards purse
    let handle_payment_hash = builder.get_handle_payment_contract_hash();
    let handle_payment_contract = builder
        .get_contract(handle_payment_hash)
        .expect("should have handle payment contract");

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
    let handle_payment_contract = builder
        .get_contract(handle_payment_hash)
        .expect("should have handle payment contract");
    let rewards_purse = handle_payment_contract
        .named_keys()
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

    let handle_payment_after = builder
        .get_contract(handle_payment_hash)
        .expect("should have handle payment contract");

    assert_eq!(
        handle_payment_after
            .named_keys()
            .get(ACCUMULATION_PURSE_KEY),
        handle_payment_contract
            .named_keys()
            .get(ACCUMULATION_PURSE_KEY),
        "keys should not change before and after deploy has been processed",
    );

    let rewards_purse = handle_payment_contract.named_keys()[ACCUMULATION_PURSE_KEY]
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
