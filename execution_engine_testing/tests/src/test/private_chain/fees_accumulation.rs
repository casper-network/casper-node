use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, MINIMUM_ACCOUNT_CREATION_BALANCE,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{
    engine_state::{engine_config::RefundHandling, Error},
    execution,
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{
        handle_payment::{self, REWARDS_PURSE_KEY},
        mint,
    },
    ApiError, RuntimeArgs, U512,
};

use crate::{
    test::private_chain::{self, ACCOUNT_1_ADDR, DEFAULT_ADMIN_ACCOUNT_ADDR},
    wasm_utils,
};

#[ignore]
#[test]
fn default_genesis_config_should_not_have_rewards_purse() {
    let genesis_request = PRODUCTION_RUN_GENESIS_REQUEST.clone();
    assert!(matches!(
        genesis_request.ee_config().refund_handling(),
        RefundHandling::Refund { .. }
    ));

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let handle_payment = builder.get_handle_payment_contract_hash();
    let handle_payment_contract = builder
        .get_contract(handle_payment)
        .expect("should have handle payment contract");

    assert!(
        !handle_payment_contract
            .named_keys()
            .contains_key(REWARDS_PURSE_KEY),
        "Found rewards purse in handle payment's named keys {:?}",
        handle_payment_contract.named_keys()
    );

    assert!(
        !handle_payment_contract
            .entry_points()
            .has_entry_point(handle_payment::METHOD_GET_REWARDS_PURSE),
        "handle payment contract should not have get rewards purse"
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

    let rewards_purse = handle_payment_contract.named_keys()[REWARDS_PURSE_KEY]
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
        handle_payment_after.named_keys().get(REWARDS_PURSE_KEY),
        handle_payment_contract.named_keys().get(REWARDS_PURSE_KEY),
        "keys should not change before and after deploy has been processed",
    );

    let rewards_purse = handle_payment_contract.named_keys()[REWARDS_PURSE_KEY]
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
fn should_withdraw_rewards_as_admin() {
    let mut builder = super::private_chain_setup();

    let handle_payment_hash = builder.get_handle_payment_contract_hash();
    let handle_payment = builder
        .get_contract(handle_payment_hash)
        .expect("should have handle payment contract");
    let rewards_purse = handle_payment.named_keys()[REWARDS_PURSE_KEY]
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
    let rewards_balance_before = builder.get_purse_balance(rewards_purse);
    assert!(!rewards_balance_before.is_zero());

    // Ensures default proposer didn't receive any funds
    let target_account = AccountHash::new([11; 32]);
    assert!(builder.get_account(target_account).is_none());

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        "withdraw_rewards.wasm",
        runtime_args! {
            "target" => target_account,
            "amount" => rewards_balance_before,
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    assert_ne!(
        builder.get_purse_balance(rewards_purse),
        rewards_balance_before,
        "we emptied out what we have in rewards purse, but it gets filled with new gas fees"
    );

    let account_1 = builder
        .get_account(target_account)
        .expect("should have account");
    assert_eq!(
        builder.get_purse_balance(account_1.main_purse()),
        rewards_balance_before
    );
}

#[ignore]
#[test]
fn should_not_allow_to_withdraw_rewards_as_user() {
    let mut builder = super::private_chain_setup();

    let handle_payment_hash = builder.get_handle_payment_contract_hash();
    let handle_payment = builder
        .get_contract(handle_payment_hash)
        .expect("should have handle payment contract");
    let rewards_purse = handle_payment.named_keys()[REWARDS_PURSE_KEY]
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
    let rewards_balance_before = builder.get_purse_balance(rewards_purse);
    assert!(!rewards_balance_before.is_zero());

    // Ensures default proposer didn't receive any funds
    let target_account = AccountHash::new([11; 32]);
    assert!(builder.get_account(target_account).is_none());

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        "withdraw_rewards.wasm",
        runtime_args! {
            "target" => target_account,
            "amount" => rewards_balance_before,
        },
    )
    .build();

    builder.exec(exec_request_2).expect_failure().commit();

    let error = builder.get_error().expect("should have error");

    assert!(
        matches!(
            error,
            Error::Exec(execution::Error::Revert(ApiError::HandlePayment(handle_payment_error))) if handle_payment_error == handle_payment::Error::SystemFunctionCalledByUserAccount as u8
        ),
        "{:?}",
        error
    );
    assert!(builder.get_account(target_account).is_none());
}
