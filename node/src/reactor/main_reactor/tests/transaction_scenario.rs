mod asertions;
mod utils;

use asertions::{
    ExecResultCost, PublicKeyBalanceChange, PublicKeyTotalMeetsAvailable, TotalSupplyChange,
    TransactionFailure, TransactionSuccessful,
};
use casper_types::{
    bytesrepr::{Bytes, ToBytes},
    execution::{RetValue, TransformKindV2},
    system::auction::{DelegatorKind, Reservation},
    testing::TestRng,
    EntityAddr, ExecutionInfo, FeeHandling, Gas, Key, PricingHandling, PricingMode, PublicKey,
    RefundHandling, StoredValue, TimeDiff, Transaction, TransactionEntryPoint,
    TransactionInvocationTarget, TransactionRuntimeParams, TransactionV1Config, U512,
};
use num_rational::Ratio;
use utils::{build_wasm_transction, RunUntilCondition, TestScenarioBuilder};

use crate::{
    reactor::main_reactor::tests::{
        transaction_scenario::asertions::{BalanceChange, ExecutionResultHasRet},
        transactions::{
            invalid_wasm_txn, ALICE_PUBLIC_KEY, ALICE_SECRET_KEY, BOB_PUBLIC_KEY, BOB_SECRET_KEY,
            CHARLIE_PUBLIC_KEY, MIN_GAS_PRICE,
        },
        ONE_MIN,
    },
    testing::LARGE_WASM_LANE_ID,
    types::transaction::transaction_v1_builder::TransactionV1Builder,
    utils::RESOURCES_PATH,
};

#[tokio::test]
async fn should_accept_transfer_without_id() {
    let mut rng = TestRng::new();
    let builder = TestScenarioBuilder::new();
    let mut test_scenario = builder.build(&mut rng).await;

    //This should be 1 mote more than the native_transfer_minimum_motes in local
    // chainspec that we use for tests
    let transfer_amount = 2_500_000_001_u64;

    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();

    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_transfer(transfer_amount, None, CHARLIE_PUBLIC_KEY.clone(), None)
            .unwrap()
            .with_initiator_addr(ALICE_PUBLIC_KEY.clone())
            .with_pricing_mode(PricingMode::Fixed {
                gas_price_tolerance: 1,
                additional_computation_factor: 0,
            })
            .with_chain_name(chain_name)
            .build()
            .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();

    test_scenario.assert(TransactionSuccessful::new(hash)).await;
}

#[tokio::test]
async fn should_native_transfer_nofee_norefund_fixed() {
    const TRANSFER_AMOUNT: u64 = 30_000_000_000;
    let mut rng = TestRng::new();
    let builder = TestScenarioBuilder::new()
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_balance_hold_interval(TimeDiff::from_seconds(5));
    let mut test_scenario = builder.build(&mut rng).await;

    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();

    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_transfer(
            TRANSFER_AMOUNT,
            None,
            CHARLIE_PUBLIC_KEY.clone(),
            Some(0xDEADBEEF),
        )
        .unwrap()
        .with_initiator_addr(ALICE_PUBLIC_KEY.clone())
        .with_pricing_mode(PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        })
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();

    let expected_transfer_gas: U512 = test_scenario.mint_const_transfer_cost().into();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;

    test_scenario
        .assert(ExecResultCost::new(
            hash,
            expected_transfer_gas,
            Gas::new(expected_transfer_gas),
        ))
        .await;

    let transfer_amount = U512::from(TRANSFER_AMOUNT);
    let transfer_amount_and_gas: U512 = transfer_amount
        .checked_add(expected_transfer_gas)
        .expect("should math");

    test_scenario
        .assert(PublicKeyBalanceChange::new(
            ALICE_PUBLIC_KEY.clone(),
            BalanceChange::Down(transfer_amount),
            BalanceChange::Down(transfer_amount_and_gas),
        ))
        .await;
    //Charlie should have the transfer amount at his disposal
    test_scenario
        .assert(PublicKeyBalanceChange::new(
            CHARLIE_PUBLIC_KEY.clone(),
            BalanceChange::Up(transfer_amount),
            BalanceChange::Up(transfer_amount),
        ))
        .await;
    // Check if the hold is released.
    let hold_release_block_height = test_scenario.get_block_height() + 9; // Block time is 1s.
    test_scenario
        .run_until(RunUntilCondition::BlockHeight {
            block_height: hold_release_block_height,
            within: ONE_MIN,
        })
        .await
        .unwrap();
    test_scenario
        .assert(PublicKeyTotalMeetsAvailable::new(ALICE_PUBLIC_KEY.clone()))
        .await;
}

#[tokio::test]
async fn erroneous_native_transfer_nofee_norefund_fixed() {
    let mut rng = TestRng::new();
    let builder = TestScenarioBuilder::new()
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_balance_hold_interval(TimeDiff::from_seconds(5));
    let mut test_scenario = builder.build(&mut rng).await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();

    let transfer_amount = test_scenario.native_transfer_minimum_motes() + 100;

    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_transfer(transfer_amount, None, CHARLIE_PUBLIC_KEY.clone(), None)
            .unwrap()
            .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
            .with_pricing_mode(PricingMode::Fixed {
                gas_price_tolerance: 1,
                additional_computation_factor: 0,
            })
            .with_chain_name(chain_name.clone())
            .build()
            .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();

    test_scenario.assert(TransactionSuccessful::new(hash)).await;

    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_transfer(
            transfer_amount + 100,
            None,
            BOB_PUBLIC_KEY.clone(),
            None,
        )
        .unwrap()
        .with_initiator_addr(CHARLIE_PUBLIC_KEY.clone())
        .with_pricing_mode(PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        })
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionFailure::new(hash)).await; // transaction should have failed.
    let expected_transfer_cost = test_scenario.mint_const_transfer_cost() as u64;
    let expected_transfer_gas: U512 = expected_transfer_cost.into();
    test_scenario
        .assert(ExecResultCost::new(
            hash,
            expected_transfer_gas,
            Gas::new(expected_transfer_gas),
        ))
        .await;
    // Even though the transaction failed, a hold must still be in place for the transfer cost.
    // The hold will show up in "available" being smaller than "total"

    let transfer_amount_x = U512::from(transfer_amount);
    let transfer_amount_y = transfer_amount_x
        .checked_sub(U512::from(expected_transfer_cost))
        .expect("should sub transfer from transfer amount");

    test_scenario
        .assert(PublicKeyBalanceChange::new(
            CHARLIE_PUBLIC_KEY.clone(),
            BalanceChange::Up(transfer_amount_x),
            BalanceChange::Up(transfer_amount_y),
        ))
        .await;
}

#[tokio::test]
async fn should_cancel_refund_for_erroneous_wasm() {
    // as a punitive measure, refunds are not issued for erroneous wasms even
    // if refunds are turned on.

    let mut rng = TestRng::new();
    let refund_ratio = Ratio::new(1, 2);
    let builder = TestScenarioBuilder::new()
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);
    let mut test_scenario = builder.build(&mut rng).await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn = build_wasm_transction(
        chain_name,
        &BOB_SECRET_KEY,
        PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        },
    );
    txn.sign(&BOB_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionFailure::new(hash)).await; // transaction should have failed.
    let expected_transaction_cost = 1_000_000_000_000_u64; // transaction gas limit for large wasms lane
    test_scenario
        .assert(ExecResultCost::new(
            hash,
            expected_transaction_cost.into(),
            Gas::new(0),
        ))
        .await;

    // transaction should have failed.
    test_scenario.assert(TransactionFailure::new(hash)).await;

    let x = BalanceChange::Down(U512::from(expected_transaction_cost));
    // Bob gets no refund because the wasm errored
    test_scenario
        .assert(PublicKeyBalanceChange::new(BOB_PUBLIC_KEY.clone(), x, x))
        .await;

    let y = BalanceChange::Up(U512::from(expected_transaction_cost));
    // Alice should get all the fee since it's set to pay to proposer
    // AND Bob didn't get a refund
    test_scenario
        .assert(PublicKeyBalanceChange::new(ALICE_PUBLIC_KEY.clone(), y, y))
        .await;
}

#[tokio::test]
async fn should_not_refund_erroneous_wasm_burn_fixed() {
    let mut rng = TestRng::new();
    let refund_ratio = Ratio::new(1, 2);
    let builder = TestScenarioBuilder::new()
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_minimum_era_height(5) // make the era longer so that the transaction doesn't land in the switch block.
        .with_balance_hold_interval(TimeDiff::from_seconds(5));
    let mut test_scenario = builder.build(&mut rng).await;
    test_scenario.setup().await.unwrap();
    let gas_limit = test_scenario
        .get_gas_limit_for_lane(LARGE_WASM_LANE_ID) // The wasm should fall in this lane
        .unwrap();
    let txn = invalid_wasm_txn(
        BOB_SECRET_KEY.clone(),
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
            additional_computation_factor: 0,
        },
    );
    let hash = txn.hash();

    let exec_infos = test_scenario.run(vec![txn]).await.unwrap();

    test_scenario.assert(TransactionFailure::new(hash)).await; // transaction should have failed.
    test_scenario
        .assert(ExecResultCost::new(hash, gas_limit.into(), Gas::new(0)))
        .await;
    // Supply shouldn't change (refund handling is burn, but the wasm was erroneous so we don't
    // calulate refund)
    test_scenario
        .assert(TotalSupplyChange::new(0, exec_infos[0].block_height))
        .await;
    // Bobs transaction was invalid. He should get NO refund. But also -
    // since no refund is calculated nothing will be burned (despite
    // RefundHandling::Burn - we don't calculate refunds for erroneous wasms)
    let gas_limit_x = BalanceChange::Down(U512::from(gas_limit));
    test_scenario
        .assert(PublicKeyBalanceChange::new(
            BOB_PUBLIC_KEY.clone(),
            gas_limit_x,
            gas_limit_x,
        ))
        .await;
    let gas_limit_y = BalanceChange::Up(U512::from(gas_limit));
    // Alice gets payed for executing the transaction since it's set to pay to proposer
    test_scenario
        .assert(PublicKeyBalanceChange::new(
            ALICE_PUBLIC_KEY.clone(),
            gas_limit_y,
            gas_limit_y,
        ))
        .await;
}

#[tokio::test]
async fn vm2_contract_can_be_called_using_by_package_hash() {
    let module_bytes = read_wasm("vm2_upgradable.wasm");
    let mut rng = TestRng::new();
    let builder = TestScenarioBuilder::new().with_enable_vm2(true);

    let mut test_scenario = builder.build(&mut rng).await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_session(
            true,
            module_bytes.into(),
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_entry_point(TransactionEntryPoint::Custom("new".to_string()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            10_u8.to_bytes().unwrap().into(),
        ))
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_pricing_mode(PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    let execution_infos = test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    let package_hash = peel_package_hash_info(execution_infos);

    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByHash(package_hash),
            "get",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        })
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    test_scenario
        .assert(ExecutionResultHasRet::new(
            hash,
            RetValue::Bytes(10_u8.to_bytes().unwrap().into()),
        ))
        .await;
}

#[tokio::test]
async fn vm2_contract_can_be_called_using_by_package_hash_with_version() {
    let module_bytes = read_wasm("vm2_upgradable.wasm");
    let mut rng = TestRng::new();
    let builder = TestScenarioBuilder::new().with_enable_vm2(true);

    let mut test_scenario = builder.build(&mut rng).await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_session(
            true,
            module_bytes.into(),
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_entry_point(TransactionEntryPoint::Custom("new".to_string()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            10_u8.to_bytes().unwrap().into(),
        ))
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_pricing_mode(PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    let execution_infos = test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    let package_hash = peel_package_hash_info(execution_infos);

    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageHash {
                addr: package_hash.into(),
                version: Some(1),
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "get",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        })
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    test_scenario
        .assert(ExecutionResultHasRet::new(
            hash,
            RetValue::Bytes(10_u8.to_bytes().unwrap().into()),
        ))
        .await;
}

#[tokio::test]
async fn vm2_contract_calling_by_hash_nonexistent_fails() {
    let module_bytes = read_wasm("vm2_upgradable.wasm");
    let mut rng = TestRng::new();
    let builder = TestScenarioBuilder::new().with_enable_vm2(true);

    let mut test_scenario = builder.build(&mut rng).await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_session(
            true,
            module_bytes.into(),
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_entry_point(TransactionEntryPoint::Custom("new".to_string()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            10_u8.to_bytes().unwrap().into(),
        ))
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_pricing_mode(PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    let execution_infos = test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    let package_hash = peel_package_hash_info(execution_infos);

    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageHash {
                addr: package_hash.into(),
                version: Some(100), //Nonexistent version
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "get",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        })
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario
        .assert(TransactionFailure::expected_error_message(
            hash,
            "no active contract",
        ))
        .await;
}

#[tokio::test]
async fn vm2_contract_calling_by_hash_after_upgrade() {
    let module_bytes = read_wasm("vm2_upgradable.wasm");
    let mut rng = TestRng::new();
    let mut test_scenario = TestScenarioBuilder::new()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_enable_vm2(true)
        .with_transaction_v1_config(TransactionV1Config::very_big_wasm_lane())
        .build(&mut rng)
        .await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_session(
            true,
            module_bytes.into(),
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_entry_point(TransactionEntryPoint::Custom("new".to_string()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            0_u8.to_bytes().unwrap().into(),
        ))
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    let execution_infos = test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    let package_hash = peel_package_hash_info(execution_infos);

    let new_code = Bytes::from(read_wasm("vm2_upgradable_v2.wasm"));
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageHash {
                addr: package_hash.into(),
                version: None,
                protocol_version_major: None,
            },
            "perform_upgrade",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            new_code.to_bytes().unwrap().into(),
        ))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageHash {
                addr: package_hash.into(),
                version: Some(2), //Existing version
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "version",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    test_scenario
        .assert(ExecutionResultHasRet::new(
            hash,
            RetValue::Bytes("v2".to_bytes().unwrap().into()),
        ))
        .await;

    // The old version (1) should be disabled by the upgrade
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageHash {
                addr: package_hash.into(),
                version: Some(1), //Existing version
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "version",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario
        .assert(TransactionFailure::expected_error_message(
            hash,
            "no active contract",
        ))
        .await;
}

#[tokio::test]
async fn vm2_contract_calling_by_name_after_upgrade() {
    let module_bytes = read_wasm("vm2_upgradable_storing_package.wasm");
    let mut rng = TestRng::new();
    let mut test_scenario = TestScenarioBuilder::new()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_enable_vm2(true)
        .with_transaction_v1_config(TransactionV1Config::very_big_wasm_lane())
        .build(&mut rng)
        .await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_session(
            true,
            module_bytes.into(),
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_entry_point(TransactionEntryPoint::Custom("new".to_string()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            0_u8.to_bytes().unwrap().into(),
        ))
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;

    let new_code = Bytes::from(read_wasm("vm2_upgradable_v2.wasm"));
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageName {
                name: "package".to_string(),
                version: None,
                protocol_version_major: None,
            },
            "perform_upgrade",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            new_code.to_bytes().unwrap().into(),
        ))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageName {
                name: "package".to_string(),
                version: Some(2), //Existing version
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "version",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    test_scenario
        .assert(ExecutionResultHasRet::new(
            hash,
            RetValue::Bytes("v2".to_bytes().unwrap().into()),
        ))
        .await;
    // The old version (1) should be disabled by the upgrade
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageName {
                name: "package".to_string(),
                version: Some(1), //Existing version
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "version",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario
        .assert(TransactionFailure::expected_error_message(
            hash,
            "no active contract",
        ))
        .await;
}

#[tokio::test]
async fn vm2_contract_calling_by_name_with_addressable_entity_after_upgrade() {
    let module_bytes = read_wasm("vm2_upgradable_storing_package.wasm");
    let mut rng = TestRng::new();
    let mut test_scenario = TestScenarioBuilder::new()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_addressable_entity(true)
        .with_enable_vm2(true)
        .with_transaction_v1_config(TransactionV1Config::very_big_wasm_lane())
        .build(&mut rng)
        .await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_session(
            true,
            module_bytes.into(),
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_entry_point(TransactionEntryPoint::Custom("new".to_string()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            0_u8.to_bytes().unwrap().into(),
        ))
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;

    let new_code = Bytes::from(read_wasm("vm2_upgradable_v2.wasm"));
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageName {
                name: "package".to_string(),
                version: None,
                protocol_version_major: None,
            },
            "perform_upgrade",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            new_code.to_bytes().unwrap().into(),
        ))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageName {
                name: "package".to_string(),
                version: Some(2), //Existing version
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "version",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    test_scenario
        .assert(ExecutionResultHasRet::new(
            hash,
            RetValue::Bytes("v2".to_bytes().unwrap().into()),
        ))
        .await;

    // The old version (1) should be disabled by the upgrade
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageName {
                name: "package".to_string(),
                version: Some(1), //Existing version
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "version",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario
        .assert(TransactionFailure::expected_error_message(
            hash,
            "no active contract",
        ))
        .await;
}

#[tokio::test]
async fn vm2_contract_calling_by_hash_with_addressable_entity_after_upgrade() {
    let module_bytes = read_wasm("vm2_upgradable.wasm");
    let mut rng = TestRng::new();
    let mut test_scenario = TestScenarioBuilder::new()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_addressable_entity(true)
        .with_enable_vm2(true)
        .with_transaction_v1_config(TransactionV1Config::very_big_wasm_lane())
        .build(&mut rng)
        .await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_session(
            true,
            module_bytes.into(),
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_entry_point(TransactionEntryPoint::Custom("new".to_string()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            0_u8.to_bytes().unwrap().into(),
        ))
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    let execution_infos = test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    let package_hash = peel_package_hash_info(execution_infos);

    let new_code = Bytes::from(read_wasm("vm2_upgradable_v2.wasm"));
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageHash {
                addr: package_hash.into(),
                version: None,
                protocol_version_major: None,
            },
            "perform_upgrade",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(
            new_code.to_bytes().unwrap().into(),
        ))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageHash {
                addr: package_hash.into(),
                version: Some(2), //Existing version
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "version",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;
    test_scenario
        .assert(ExecutionResultHasRet::new(
            hash,
            RetValue::Bytes("v2".to_bytes().unwrap().into()),
        ))
        .await;

    // The old version (1) should be disabled by the upgrade
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_targeting_stored(
            TransactionInvocationTarget::ByPackageHash {
                addr: package_hash.into(),
                version: Some(1), //Existing version
                protocol_version_major: Some(test_scenario.get_protocol_version().destructure().0),
            },
            "version",
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value: 0,
                seed: None,
                bundle_data: None,
            },
        )
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_transaction_args(casper_types::TransactionArgs::Bytesrepr(vec![].into()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario
        .assert(TransactionFailure::expected_error_message(
            hash,
            "no active contract",
        ))
        .await;
}

#[tokio::test]
async fn native_add_bid_should_fail_when_minimum_delegation_rate_not_met() {
    let mut rng = TestRng::new();
    let mut test_scenario = TestScenarioBuilder::new()
        .with_minimum_delegation_rate(20)
        .build(&mut rng)
        .await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_add_bid(
            ALICE_PUBLIC_KEY.clone(),
            19,
            100_000_000_000_u64,
            None,
            None,
            None,
        )
        .unwrap()
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario
        .assert(TransactionFailure::expected_error_message(
            hash,
            "ApiError::AuctionError(DelegationRateTooSmall) [64576]",
        ))
        .await;
}

#[tokio::test]
async fn native_add_bid_should_fail_when_minimum_delegation_rate_not_met_in_reservation() {
    let mut rng = TestRng::new();
    let mut test_scenario = TestScenarioBuilder::new()
        .with_minimum_delegation_rate(20)
        .build(&mut rng)
        .await;
    let chain_name = test_scenario.chain_name();
    test_scenario.setup().await.unwrap();
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_add_bid(
            ALICE_PUBLIC_KEY.clone(),
            20,
            100_000_000_000_u64,
            None,
            None,
            Some(1),
        )
        .unwrap()
        .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: 100_000_000_000_u64,
            gas_price_tolerance: 1,
            standard_payment: true,
        })
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario.assert(TransactionSuccessful::new(hash)).await;

    // Try reserve a slot with to little delegation rate
    let reservations = vec![Reservation::new(
        ALICE_PUBLIC_KEY.clone(),
        DelegatorKind::PublicKey(BOB_PUBLIC_KEY.clone()),
        19,
    )];
    let mut txn: Transaction = Transaction::from(
        TransactionV1Builder::new_reserve_slot(reservations)
            .unwrap()
            .with_initiator_addr(PublicKey::from(ALICE_SECRET_KEY.as_ref()))
            .with_pricing_mode(PricingMode::PaymentLimited {
                payment_amount: 100_000_000_000_u64,
                gas_price_tolerance: 1,
                standard_payment: true,
            })
            .with_chain_name(chain_name.clone())
            .build()
            .unwrap(),
    );
    txn.sign(&ALICE_SECRET_KEY);
    let hash = txn.hash();
    test_scenario.run(vec![txn]).await.unwrap();
    test_scenario
        .assert(TransactionFailure::expected_error_message(
            hash,
            "Auction error: Delegation rate too small",
        ))
        .await;
}

fn peel_package_hash_info(execution_infos: Vec<ExecutionInfo>) -> [u8; 32] {
    let ei = execution_infos
        .first()
        .expect("Expecting at least one ExecutionInfo");
    let er = ei
        .execution_result
        .clone()
        .expect("Expected execution result");
    match er {
        casper_types::execution::ExecutionResult::V1(_) => {
            panic!("Shouldn't happen")
        }
        casper_types::execution::ExecutionResult::V2(execution_result_v2) => {
            let effects = execution_result_v2.effects;
            let transforms = effects.transforms();
            *transforms
                .iter()
                .filter_map(|el| {
                    if matches!(
                        el.kind(),
                        TransformKindV2::Write(StoredValue::SmartContract(_))
                    ) || matches!(
                        el.kind(),
                        TransformKindV2::Write(StoredValue::ContractPackage(_))
                    ) {
                        Some(match el.key() {
                            Key::Hash(package_hash) => *package_hash,
                            Key::AddressableEntity(EntityAddr::SmartContract(package_hash)) => {
                                *package_hash
                            }
                            Key::Package(package_hash) => package_hash.value(),
                            _ => {
                                todo!()
                            }
                        })
                    } else {
                        None
                    }
                })
                .collect::<Vec<[u8; 32]>>()
                .first()
                .expect("Expected to find key under which the package was stored")
        }
    }
}

fn read_wasm(file_name: &str) -> Vec<u8> {
    let contract_file = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(file_name);
    std::fs::read(contract_file).expect("couldn't read module bytes")
}
