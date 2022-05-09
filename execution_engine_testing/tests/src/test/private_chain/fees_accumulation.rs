use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST,
    MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::engine_state::engine_config::FeeElimination;
use casper_types::{
    runtime_args,
    system::mint::{self, REWARDS_PURSE_KEY},
    RuntimeArgs, U512,
};

use crate::{
    test::private_chain::{self, ACCOUNT_1_ADDR, DEFAULT_ADMIN_ACCOUNT_ADDR},
    wasm_utils,
};

#[ignore]
#[test]
fn default_genesis_config_should_not_have_rewards_purse() {
    let genesis_request = DEFAULT_RUN_GENESIS_REQUEST.clone();
    assert!(matches!(
        genesis_request.ee_config().fee_elimination(),
        FeeElimination::Refund { .. }
    ));

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let mint_hash = builder.get_mint_contract_hash();
    let mint_contract = builder
        .get_contract(mint_hash)
        .expect("should have mint contract");

    assert!(
        !mint_contract.named_keys().contains_key(REWARDS_PURSE_KEY),
        "Found rewards purse in mint's named keys {:?}",
        mint_contract.named_keys()
    );
}

#[ignore]
#[test]
fn should_finalize_and_accumulate_rewards_purse() {
    let mut builder = private_chain::setup_genesis_only();

    let mint_hash = builder.get_mint_contract_hash();
    let mint_contract_1 = builder
        .get_contract(mint_hash)
        .expect("should have mint contract");

    let rewards_purse_key = mint_contract_1
        .named_keys()
        .get(REWARDS_PURSE_KEY)
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

    let mint_contract_2 = builder
        .get_contract(mint_hash)
        .expect("should have mint contract");

    assert_eq!(
        mint_contract_1.named_keys(),
        mint_contract_2.named_keys(),
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

    let mint_contract_3 = builder
        .get_contract(mint_hash)
        .expect("should have mint contract");

    assert_eq!(
        mint_contract_1.named_keys(),
        mint_contract_2.named_keys(),
        "none of the named keys should change before and after execution"
    );
    assert_eq!(
        mint_contract_2.named_keys(),
        mint_contract_3.named_keys(),
        "none of the named keys should change before and after execution"
    );
}

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

    let exec_request_proposer = exec_request.proposer.clone();

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

    // Ensures default proposer didn't receive any funds
    let proposer_account = builder
        .get_account(exec_request_proposer.to_account_hash())
        .expect("should have proposer account");

    assert_eq!(
        builder.get_purse_balance(proposer_account.main_purse()),
        U512::zero()
    );
}
