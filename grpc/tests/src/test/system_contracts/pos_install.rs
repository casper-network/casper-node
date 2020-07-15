#![allow(dead_code)]
#![allow(unused_imports)]

use std::collections::BTreeMap;

use engine_test_support::{
    internal::{
        exec_with_return, ExecuteRequestBuilder, WasmTestBuilder, DEFAULT_BLOCK_TIME,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use node::{components::contract_runtime::core::engine_state::EngineConfig, types::DeployHash};
use types::{
    account::AccountHash, contracts::NamedKeys, runtime_args, ContractHash, ContractPackageHash,
    Key, RuntimeArgs, URef, U512,
};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const TRANSFER_AMOUNT: u64 = 250_000_000 + 1000;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);
const DEPLOY_HASH_2: [u8; 32] = [2u8; 32];
const N_VALIDATORS: u8 = 5;

// one named_key for each validator and three for the purses
const EXPECTED_KNOWN_KEYS_LEN: usize = (N_VALIDATORS as usize) + 3;

const POS_BONDING_PURSE: &str = "pos_bonding_purse";
const POS_PAYMENT_PURSE: &str = "pos_payment_purse";
const POS_REWARDS_PURSE: &str = "pos_rewards_purse";

const ARG_MINT_PACKAGE_HASH: &str = "mint_contract_package_hash";
const ARG_GENESIS_VALIDATORS: &str = "genesis_validators";

#[ignore]
#[test]
fn should_run_pos_install_contract() {
    let mut builder = WasmTestBuilder::default();
    let engine_config = EngineConfig::new()
        .with_use_system_contracts(cfg!(feature = "use-system-contracts"))
        .with_enable_bonding(cfg!(feature = "enable-bonding"));

    let exec_request = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { "target" =>SYSTEM_ADDR, "amount" => U512::from(TRANSFER_AMOUNT) },
    )
    .build();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);
    builder.exec(exec_request).commit().expect_success();

    let mint_hash = builder.get_mint_contract_hash();

    let mint_package_stored_value = builder
        .query(None, mint_hash.into(), &[])
        .expect("should query mint hash");
    let mint_package = mint_package_stored_value
        .as_contract()
        .expect("should be contract");

    let mint_package_hash = mint_package.contract_package_hash();

    let genesis_validators: BTreeMap<AccountHash, U512> = (1u8..=N_VALIDATORS)
        .map(|i| (AccountHash::new([i; 32]), U512::from(i)))
        .collect();

    let total_bond = genesis_validators.values().fold(U512::zero(), |x, y| x + y);

    let res = exec_with_return::exec(
        engine_config,
        &mut builder,
        SYSTEM_ADDR,
        "pos_install.wasm",
        DEFAULT_BLOCK_TIME,
        DEPLOY_HASH_2,
        "install",
        runtime_args! {
            ARG_MINT_PACKAGE_HASH => mint_package_hash,
            ARG_GENESIS_VALIDATORS => genesis_validators,
        },
        vec![],
    );
    let ((_pos_package_hash, pos_hash), _ret_urefs, effect): (
        (ContractPackageHash, ContractHash),
        _,
        _,
    ) = res.expect("should run successfully");

    let prestate = builder.get_post_state_hash();
    builder.commit_effects(prestate, effect.transforms);

    // should return a hash
    //assert_eq!(ret_value, ret_urefs[0]);

    // should have written a contract under that uref
    let contract = builder
        .get_contract(pos_hash)
        .expect("should have a contract");
    let named_keys = contract.named_keys();

    assert_eq!(named_keys.len(), EXPECTED_KNOWN_KEYS_LEN);

    // bonding purse has correct balance
    let bonding_purse = get_purse(named_keys, POS_BONDING_PURSE).expect(
        "should find bonding purse in
    named_keys",
    );

    let bonding_purse_balance = builder.get_purse_balance(bonding_purse);
    assert_eq!(bonding_purse_balance, total_bond);

    // payment purse has correct balance
    let payment_purse = get_purse(named_keys, POS_PAYMENT_PURSE).expect(
        "should find payment purse in
    named_keys",
    );

    let payment_purse_balance = builder.get_purse_balance(payment_purse);
    assert_eq!(payment_purse_balance, U512::zero());

    // rewards purse has correct balance
    let rewards_purse = get_purse(named_keys, POS_REWARDS_PURSE).expect(
        "should find rewards purse in
    named_keys",
    );

    let rewards_purse_balance = builder.get_purse_balance(rewards_purse);
    assert_eq!(rewards_purse_balance, U512::zero());
}

fn get_purse(named_keys: &NamedKeys, name: &str) -> Option<URef> {
    named_keys
        .get(name)
        .expect("should have named key")
        .into_uref()
}
