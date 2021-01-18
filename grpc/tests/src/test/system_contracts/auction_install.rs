use std::collections::BTreeMap;

use casper_engine_test_support::{
    internal::{
        exec_with_return, ExecuteRequestBuilder, WasmTestBuilder, DEFAULT_AUCTION_DELAY,
        DEFAULT_BLOCK_TIME, DEFAULT_INITIAL_ERA_ID, DEFAULT_LOCKED_FUNDS_PERIOD,
        DEFAULT_RUN_GENESIS_REQUEST, DEFAULT_UNBONDING_DELAY, DEFAULT_VALIDATOR_SLOTS,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::core::engine_state::EngineConfig;
use casper_types::{
    account::AccountHash,
    auction::{
        ARG_AUCTION_DELAY, ARG_GENESIS_VALIDATORS, ARG_INITIAL_ERA_ID, ARG_LOCKED_FUNDS_PERIOD,
        ARG_MINT_CONTRACT_PACKAGE_HASH, ARG_UNBONDING_DELAY, ARG_VALIDATOR_SLOTS,
        AUCTION_DELAY_KEY, BIDS_KEY, DELEGATOR_REWARD_PURSE_KEY, ERA_ID_KEY,
        LOCKED_FUNDS_PERIOD_KEY, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, UNBONDING_PURSES_KEY,
        VALIDATOR_REWARD_PURSE_KEY,
    },
    runtime_args, ContractHash, DeployHash, RuntimeArgs, U512,
};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const TRANSFER_AMOUNT: u64 = 250_000_000 + 1000;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);
const DEPLOY_HASH_2: DeployHash = DeployHash::new([2u8; 32]);

const EXPECTED_KNOWN_KEYS_LEN: usize = 10;

#[ignore]
#[test]
fn should_run_auction_install_contract() {
    let mut builder = WasmTestBuilder::default();
    let engine_config =
        EngineConfig::new().with_use_system_contracts(cfg!(feature = "use-system-contracts"));

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);
    builder.exec(exec_request).commit().expect_success();

    let auction_hash = builder.get_auction_contract_hash();

    let auction_stored_value = builder
        .query(None, auction_hash.into(), &[])
        .expect("should query auction hash");
    let auction = auction_stored_value
        .as_contract()
        .expect("should be contract");

    let mint_hash = builder.get_mint_contract_hash();

    let mint_stored_value = builder
        .query(None, mint_hash.into(), &[])
        .expect("should query mint hash");
    let mint = mint_stored_value.as_contract().expect("should be contract");

    let _auction_hash = auction.contract_package_hash();

    let genesis_validators: BTreeMap<casper_types::PublicKey, U512> = BTreeMap::new();

    let res = exec_with_return::exec(
        engine_config,
        &mut builder,
        SYSTEM_ADDR,
        "auction_install.wasm",
        DEFAULT_BLOCK_TIME,
        DEPLOY_HASH_2,
        "install",
        runtime_args! {
            ARG_MINT_CONTRACT_PACKAGE_HASH => mint.contract_package_hash(),
            ARG_GENESIS_VALIDATORS => genesis_validators,
            ARG_VALIDATOR_SLOTS => DEFAULT_VALIDATOR_SLOTS,
            ARG_AUCTION_DELAY => DEFAULT_AUCTION_DELAY,
            ARG_LOCKED_FUNDS_PERIOD => DEFAULT_LOCKED_FUNDS_PERIOD,
            ARG_UNBONDING_DELAY => DEFAULT_UNBONDING_DELAY,
            ARG_INITIAL_ERA_ID => DEFAULT_INITIAL_ERA_ID,
        },
        vec![],
    );
    let (auction_hash, _ret_urefs, effect): (ContractHash, _, _) =
        res.expect("should run successfully");

    let prestate = builder.get_post_state_hash();
    builder.commit_effects(prestate, effect.transforms);

    // should have written a contract under that uref
    let contract = builder
        .get_contract(auction_hash)
        .expect("should have a contract");
    let named_keys = contract.named_keys();

    assert_eq!(named_keys.len(), EXPECTED_KNOWN_KEYS_LEN);

    assert!(named_keys.contains_key(BIDS_KEY));
    assert!(named_keys.contains_key(AUCTION_DELAY_KEY));
    assert!(named_keys.contains_key(LOCKED_FUNDS_PERIOD_KEY));
    assert!(named_keys.contains_key(ERA_ID_KEY));
    assert!(named_keys.contains_key(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY));
    assert!(named_keys.contains_key(UNBONDING_PURSES_KEY));
    assert!(named_keys.contains_key(DELEGATOR_REWARD_PURSE_KEY));
    assert!(named_keys.contains_key(VALIDATOR_REWARD_PURSE_KEY));
}
