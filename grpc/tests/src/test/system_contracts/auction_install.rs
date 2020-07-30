use casperlabs_engine_test_support::{
    internal::{
        exec_with_return, ExecuteRequestBuilder, WasmTestBuilder, DEFAULT_BLOCK_TIME,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casperlabs_node::components::contract_runtime::core::engine_state::EngineConfig;
use casperlabs_types::{account::AccountHash, runtime_args, ContractHash, RuntimeArgs, U512};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const TRANSFER_AMOUNT: u64 = 250_000_000 + 1000;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);
const DEPLOY_HASH_2: [u8; 32] = [2u8; 32];

// one named_key for each validator and three for the purses
const EXPECTED_KNOWN_KEYS_LEN: usize = 4;

const FOUNDER_VALIDATORS_KEY: &str = "founder_validators";
const ACTIVE_BIDS_KEY: &str = "active_bids";
const DELEGATORS_KEY: &str = "delegators";
const ERA_VALIDATORS_KEY: &str = "era_validators";

#[ignore]
#[test]
fn should_run_auction_install_contract() {
    let mut builder = WasmTestBuilder::default();
    let engine_config = EngineConfig::new()
        .with_use_system_contracts(cfg!(feature = "use-system-contracts"))
        .with_enable_bonding(cfg!(feature = "enable-bonding"));

    let exec_request = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
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

    let _auction_hash = auction.contract_package_hash();

    let res = exec_with_return::exec(
        engine_config,
        &mut builder,
        SYSTEM_ADDR,
        "auction_install.wasm",
        DEFAULT_BLOCK_TIME,
        DEPLOY_HASH_2,
        "install",
        runtime_args! {},
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

    assert!(named_keys.contains_key(FOUNDER_VALIDATORS_KEY));
    assert!(named_keys.contains_key(ACTIVE_BIDS_KEY));
    assert!(named_keys.contains_key(DELEGATORS_KEY));
    assert!(named_keys.contains_key(ERA_VALIDATORS_KEY));
}
