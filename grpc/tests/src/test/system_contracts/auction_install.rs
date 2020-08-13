use casperlabs_engine_test_support::{
    internal::{
        exec_with_return, ExecuteRequestBuilder, WasmTestBuilder, DEFAULT_BLOCK_TIME,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casperlabs_node::components::contract_runtime::core::engine_state::EngineConfig;
use casperlabs_types::{
    account::AccountHash,
    auction::{
        ACTIVE_BIDS_KEY, DELEGATORS_KEY, ERA_ID_KEY, ERA_VALIDATORS_KEY, FOUNDING_VALIDATORS_KEY,
    },
    runtime_args, ContractHash, RuntimeArgs, U512,
};
use std::collections::BTreeMap;

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const TRANSFER_AMOUNT: u64 = 250_000_000 + 1000;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);
const DEPLOY_HASH_2: [u8; 32] = [2u8; 32];

// one named_key for each validator and three for the purses
const EXPECTED_KNOWN_KEYS_LEN: usize = 5;

#[ignore]
#[test]
fn should_run_auction_install_contract() {
    let mut builder = WasmTestBuilder::default();
    let engine_config = EngineConfig::new()
        .with_use_system_contracts(cfg!(feature = "use-system-contracts"))
        .with_enable_bonding(cfg!(feature = "enable-bonding"));

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

    let genesis_validators: BTreeMap<casperlabs_types::PublicKey, U512> = BTreeMap::new();

    let res = exec_with_return::exec(
        engine_config,
        &mut builder,
        SYSTEM_ADDR,
        "auction_install.wasm",
        DEFAULT_BLOCK_TIME,
        DEPLOY_HASH_2,
        "install",
        runtime_args! {
            "mint_contract_package_hash" => mint.contract_package_hash(),
            "genesis_validators" => genesis_validators,
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

    assert!(named_keys.contains_key(FOUNDING_VALIDATORS_KEY));
    assert!(named_keys.contains_key(ACTIVE_BIDS_KEY));
    assert!(named_keys.contains_key(DELEGATORS_KEY));
    assert!(named_keys.contains_key(ERA_VALIDATORS_KEY));
    assert!(named_keys.contains_key(ERA_ID_KEY));
}
