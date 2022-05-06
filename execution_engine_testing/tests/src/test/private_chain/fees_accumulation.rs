use casper_engine_test_support::ExecuteRequestBuilder;
use casper_types::{system::mint::REWARDS_PURSE_KEY, RuntimeArgs};

use crate::{test::private_chain::DEFAULT_ADMIN_ACCOUNT_ADDR, wasm_utils};

#[ignore]
#[test]
fn should_accumulate_deploy_fees() {
    let mut builder = super::private_chain_setup();

    // Check mint has rewards purse
    let mint_hash = builder.get_mint_contract_hash();
    let mint_contract = builder
        .get_contract(mint_hash)
        .expect("should have mint contract");

    let rewards_purse = mint_contract.named_keys()[REWARDS_PURSE_KEY]
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

    builder.exec(exec_request).expect_success().commit();

    let mint_contract_after = builder
        .get_contract(mint_hash)
        .expect("should have mint contract");

    assert_eq!(
        mint_contract_after.named_keys().get(REWARDS_PURSE_KEY),
        mint_contract.named_keys().get(REWARDS_PURSE_KEY),
        "keys should not change before and after deploy has been processed",
    );

    let rewards_purse = mint_contract.named_keys()[REWARDS_PURSE_KEY]
        .into_uref()
        .expect("should be uref");
    let rewards_balance_after = builder.get_purse_balance(rewards_purse);
    assert!(
        rewards_balance_after > rewards_balance_before,
        "rewards balance should increase"
    );
}
