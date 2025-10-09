use std::{sync::Arc, time::Duration};

use casper_types::{
    testing::TestRng, Chainspec, EraId, PricingHandling, PricingMode, PublicKey, SecretKey,
    TimeDiff, Transaction, TransactionV1Config, U512,
};

use crate::{
    reactor::main_reactor::tests::{
        configs_override::ConfigsOverride, fixture::TestFixture, initial_stakes::InitialStakes,
        ERA_ONE, ERA_TWO, ERA_ZERO, ONE_MIN, TEN_SECS, THIRTY_SECS,
    },
    types::transaction::transaction_v1_builder::TransactionV1Builder,
};

#[allow(clippy::enum_variant_names)]
enum GasPriceScenario {
    SlotUtilization,
    SizeUtilization(u32),
    GasConsumptionUtilization(u64),
}

async fn run_gas_price_scenario(gas_price_scenario: GasPriceScenario) {
    let mut rng = TestRng::new();
    let alice_stake = 200_000_000_000_u64;
    let bob_stake = 300_000_000_000_u64;
    let charlie_stake = 300_000_000_000_u64;
    let initial_stakes: Vec<(U512, U512)> = vec![
        (U512::from(u64::MAX), alice_stake.into()),
        (U512::from(u64::MAX), bob_stake.into()),
        (U512::from(u64::MAX), charlie_stake.into()),
    ];

    let mut secret_keys: Vec<Arc<SecretKey>> = (0..3)
        .map(|_| Arc::new(SecretKey::random(&mut rng)))
        .collect();

    let stakes = secret_keys
        .iter()
        .zip(initial_stakes)
        .map(|(secret_key, (bal, stake))| (PublicKey::from(secret_key.as_ref()), (bal, stake)))
        .collect();

    let non_validating_secret_key = SecretKey::random(&mut rng);
    let non_validating_public_key = PublicKey::from(&non_validating_secret_key);
    secret_keys.push(Arc::new(non_validating_secret_key));

    let min_gas_price: u8 = 1;
    let max_gas_price: u8 = 3;

    let spec_override = match gas_price_scenario {
        GasPriceScenario::SlotUtilization => {
            let mut transaction_config = TransactionV1Config::default();
            transaction_config.native_mint_lane.max_transaction_count = 1;
            ConfigsOverride::default().with_transaction_v1_config(transaction_config)
        }
        GasPriceScenario::SizeUtilization(block_size) => {
            ConfigsOverride::default().with_block_size(block_size)
        }
        GasPriceScenario::GasConsumptionUtilization(gas_limit) => {
            ConfigsOverride::default().with_block_gas_limit(gas_limit)
        }
    }
    .with_lower_threshold(5u64)
    .with_upper_threshold(10u64)
    .with_minimum_era_height(5)
    .with_min_gas_price(min_gas_price)
    .with_max_gas_price(max_gas_price);

    let mut fixture =
        TestFixture::new_with_keys(rng, secret_keys, stakes, Some(spec_override)).await;

    assert_eq!(
        min_gas_price,
        fixture.chainspec.vacancy_config.min_gas_price
    );
    assert_eq!(
        max_gas_price,
        fixture.chainspec.vacancy_config.max_gas_price
    );

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);

    fixture
        .run_until_stored_switch_block_header(ERA_ZERO, ONE_MIN)
        .await;

    let mut switch_block = fixture.switch_block(ERA_ZERO);
    let mut next_gas_price = switch_block
        .era_end()
        .expect("this is a switch block")
        .next_era_gas_price();
    assert_eq!(next_gas_price, min_gas_price, "price should start at min");

    let mut current_era = switch_block.era_id();
    let chain_name = fixture.chainspec.network_config.name.clone();

    assert_eq!(current_era, EraId::new(0), "current era should be genesis");
    // Run the network at load for at least 5 eras.
    for idx in 1..=max_gas_price {
        let rng = fixture.rng_mut();
        let target_public_key = PublicKey::random(rng);
        let fixed_native_mint_transaction =
            TransactionV1Builder::new_transfer(10_000_000_000u64, None, target_public_key, None)
                .expect("must get builder")
                .with_chain_name(chain_name.clone())
                .with_secret_key(&alice_secret_key)
                .with_ttl(TimeDiff::from_seconds(120 * 10))
                .with_pricing_mode(PricingMode::Fixed {
                    gas_price_tolerance: max_gas_price,
                    additional_computation_factor: 0,
                })
                .build()
                .expect("must get transaction");

        let txn = Transaction::V1(fixed_native_mint_transaction);
        fixture.inject_transaction(txn).await;
        let next_era = current_era.successor();
        fixture
            .run_until_stored_switch_block_header(next_era, ONE_MIN)
            .await;
        switch_block = fixture.switch_block(EraId::new(idx as u64));
        next_gas_price = switch_block
            .era_end()
            .expect("this is a switch block")
            .next_era_gas_price();
        let expected = {
            let mut val = min_gas_price + idx;
            if val > max_gas_price {
                val = max_gas_price;
            }
            val
        };
        assert_eq!(
            next_gas_price, expected,
            "price goes up by 1 each era (with current settings), up to the max"
        );
        current_era = next_era;
    }

    assert_eq!(
        next_gas_price, max_gas_price,
        "calculated gas price should match the max gas price"
    );
    assert_eq!(
        current_era,
        EraId::new(max_gas_price as u64),
        "we cranked a number of eras to walk up to the max price"
    );

    let gas_price_for_non_validating_node =
        fixture.get_block_gas_price_by_public_key(Some(&non_validating_public_key));
    assert_eq!(max_gas_price, gas_price_for_non_validating_node);
    let rng = fixture.rng_mut();
    let target_public_key = PublicKey::random(rng);

    let holds_before = fixture.check_account_balance_hold_at_tip(alice_public_key.clone());
    let amount = 10_000_000_000u64;

    let fixed_native_mint_transaction =
        TransactionV1Builder::new_transfer(amount, None, target_public_key, None)
            .expect("must get builder")
            .with_chain_name(chain_name)
            .with_secret_key(&alice_secret_key)
            .with_pricing_mode(PricingMode::Fixed {
                gas_price_tolerance: max_gas_price,
                additional_computation_factor: 0,
            })
            .build()
            .expect("must get transaction");

    let txn = Transaction::V1(fixed_native_mint_transaction);
    let txn_hash = txn.hash();

    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, Duration::from_secs(20))
        .await;

    let holds_after = fixture.check_account_balance_hold_at_tip(alice_public_key.clone());

    let current_gas_price = fixture
        .highest_complete_block()
        .maybe_current_gas_price()
        .expect("must have gas price");

    let cost = match fixture.chainspec.core_config.pricing_handling {
        PricingHandling::PaymentLimited => 0,
        PricingHandling::Fixed => {
            fixture.chainspec.system_costs_config.mint_costs().transfer * (current_gas_price as u32)
        }
    };

    assert_eq!(holds_after, holds_before + U512::from(cost));

    // Run the network at zero load and ensure the value falls back to the floor.
    for _ in 0..5 {
        let next_era = current_era.successor();
        fixture
            .run_until_stored_switch_block_header(next_era, ONE_MIN)
            .await;
        current_era = next_era;
    }

    let expected_gas_price = fixture.chainspec.vacancy_config.min_gas_price;
    let actual_gas_price = fixture.get_current_era_price();
    assert_eq!(actual_gas_price, expected_gas_price);
}

#[tokio::test]
async fn should_raise_gas_price_to_ceiling_and_reduce_to_floor_based_on_slot_utilization() {
    let scenario = GasPriceScenario::SlotUtilization;
    run_gas_price_scenario(scenario).await
}

#[tokio::test]
async fn should_raise_gas_price_to_ceiling_and_reduce_to_floor_based_on_gas_consumption() {
    let gas_limit = Chainspec::default()
        .system_costs_config
        .mint_costs()
        .transfer as u64;
    let scenario = GasPriceScenario::GasConsumptionUtilization(gas_limit);
    run_gas_price_scenario(scenario).await
}

#[tokio::test]
async fn should_raise_gas_price_to_ceiling_and_reduce_to_floor_based_on_size_consumption() {
    // The size of a native transfer is roughly 300 ~ 400 bytes
    let size_limit = 600u32;
    let scenario = GasPriceScenario::SizeUtilization(size_limit);
    run_gas_price_scenario(scenario).await
}

#[tokio::test]
async fn gas_price_calc_should_not_stall_network() {
    let initial_stakes = InitialStakes::AllEqual {
        count: 5,
        stake: 1_000_000_000,
    };

    let min_gas_price: u8 = 1;
    let max_gas_price: u8 = 3;
    let minimum_era_height = 5;

    let mut transaction_config = TransactionV1Config::default();
    transaction_config.native_mint_lane.max_transaction_count = 1;

    let spec_override = ConfigsOverride::default()
        .with_transaction_v1_config(transaction_config)
        .with_lower_threshold(5u64)
        .with_upper_threshold(10u64)
        .with_minimum_era_height(minimum_era_height)
        .with_idle_tolerance(TimeDiff::from_seconds(1))
        .with_min_gas_price(min_gas_price)
        .with_max_gas_price(max_gas_price);

    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;

    // Run through the first era.
    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;

    let chain_name = fixture.chainspec.network_config.name.clone();
    let secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);

    let rng = fixture.rng_mut();
    let target_public_key = PublicKey::random(rng);
    let node_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let node_public_key = PublicKey::from(&*node_secret_key);

    let fixed_native_mint_transaction = TransactionV1Builder::new_transfer(
        10_000_000_000u64,
        None,
        target_public_key.clone(),
        None,
    )
    .expect("must get builder")
    .with_chain_name(chain_name.clone())
    .with_secret_key(&secret_key)
    .with_ttl(TimeDiff::from_seconds(120 * 10))
    .with_pricing_mode(PricingMode::Fixed {
        gas_price_tolerance: max_gas_price,
        additional_computation_factor: 0,
    })
    .build()
    .expect("must get transaction");

    let txn = Transaction::V1(fixed_native_mint_transaction);
    let txn_hash = txn.hash();
    fixture.inject_transaction(txn).await;

    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    let block_hash = *fixture.highest_complete_block().hash();

    fixture.delete_block_utilization_score_by_block_hash_in_node(&node_public_key, block_hash);

    fixture
        .run_until_stored_switch_block_header(ERA_TWO, ONE_MIN)
        .await;

    let gas_price = fixture
        .switch_block(ERA_TWO)
        .header()
        .era_end()
        .unwrap()
        .next_era_gas_price();

    fixture
        .check_gas_price_for_nodes(gas_price, THIRTY_SECS)
        .await;
}
