use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, StepRequestBuilder, UpgradeRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_AUCTION_DELAY, DEFAULT_GENESIS_TIMESTAMP_MILLIS,
    DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PROTOCOL_VERSION, MINIMUM_ACCOUNT_CREATION_BALANCE,
    PRODUCTION_RUN_GENESIS_REQUEST, TIMESTAMP_MILLIS_INCREMENT,
};
use casper_execution_engine::core::engine_state::{
    EngineConfig, RewardItem, DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
};
use casper_types::{
    runtime_args,
    system::{
        auction::{self, DelegationRate, BLOCK_REWARD, INITIAL_ERA_ID},
        mint,
    },
    EraId, ProtocolVersion, PublicKey, RuntimeArgs, SecretKey, U256, U512,
};

const VALIDATOR_STAKE: u64 = 1_000_000_000;

const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});

fn generate_secret_keys() -> impl Iterator<Item = SecretKey> {
    (1u64..).map(|i| {
        let u256 = U256::from(i);
        let mut u256_bytes = [0u8; 32];
        u256.to_big_endian(&mut u256_bytes);
        SecretKey::ed25519_from_bytes(&u256_bytes).expect("should create secret key")
    })
}

fn generate_public_keys() -> impl Iterator<Item = PublicKey> {
    generate_secret_keys().map(|secret_key| PublicKey::from(&secret_key))
}

#[ignore]
#[test]
fn regression_20220221_should_distribute_to_many_validators() {
    // distribute funds in a scenario where validator slots is greater than or equal to max runtime
    // stack height

    let mut public_keys = generate_public_keys();

    let fund_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => PublicKey::System,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let mut upgrade_request = UpgradeRequestBuilder::default()
        .with_new_validator_slots(DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT + 1)
        .with_pre_state_hash(builder.get_post_state_hash())
        .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
        .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .build();
    let engine_config = EngineConfig::default();
    builder.upgrade_with_upgrade_request(engine_config, &mut upgrade_request);

    builder.exec(fund_request).expect_success().commit();

    // Add validators
    for _ in 0..DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT {
        let public_key = public_keys.next().unwrap();

        let transfer_request = ExecuteRequestBuilder::transfer(
            *DEFAULT_ACCOUNT_ADDR,
            runtime_args! {
                mint::ARG_TARGET => public_key.to_account_hash(),
                mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE / 10),
                mint::ARG_ID => <Option<u64>>::None,
            },
        )
        .with_protocol_version(*NEW_PROTOCOL_VERSION)
        .build();

        builder.exec(transfer_request).commit().expect_success();

        let delegation_rate: DelegationRate = 10;

        let session_args = runtime_args! {
            auction::ARG_PUBLIC_KEY => public_key.clone(),
            auction::ARG_AMOUNT => U512::from(VALIDATOR_STAKE),
            auction::ARG_DELEGATION_RATE => delegation_rate,
        };

        let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
            public_key.to_account_hash(),
            builder.get_auction_contract_hash(),
            auction::METHOD_ADD_BID,
            session_args,
        )
        .with_protocol_version(*NEW_PROTOCOL_VERSION)
        .build();

        builder.exec(execute_request).expect_success().commit();
    }

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    for _ in 0..=DEFAULT_AUCTION_DELAY {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let era_validators = builder.get_era_validators();

    assert!(!era_validators.is_empty());

    let (era_id, trusted_era_validators) = era_validators
        .into_iter()
        .last()
        .expect("should have last element");
    assert!(era_id > INITIAL_ERA_ID, "{}", era_id);

    let mut step_request = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(*NEW_PROTOCOL_VERSION)
        // Next era id is used for returning future era validators, which we don't need to inspect
        // in this test.
        .with_next_era_id(era_id)
        .with_era_end_timestamp_millis(timestamp_millis);

    assert_eq!(
        trusted_era_validators.len(),
        DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT as usize
    );

    for (public_key, _stake) in trusted_era_validators.clone().into_iter() {
        let reward_amount = BLOCK_REWARD / trusted_era_validators.len() as u64;
        step_request = step_request.with_reward_item(RewardItem::new(public_key, reward_amount));
    }

    let step_request = step_request.build();

    builder.step(step_request).expect("should run step");

    builder.run_auction(timestamp_millis, Vec::new());
}
