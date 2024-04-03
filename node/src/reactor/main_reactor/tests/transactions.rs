use super::*;

use casper_types::{bytesrepr::Bytes, execution::ExecutionResultV1, TransactionSessionKind};

async fn transfer_to_account<A: Into<U512>>(
    fixture: &mut TestFixture,
    amount: A,
    from: &SecretKey,
    to: PublicKey,
    pricing: PricingMode,
    transfer_id: Option<u64>,
) -> (TransactionHash, u64, ExecutionResult) {
    let chain_name = fixture.chainspec.network_config.name.clone();

    let mut txn = Transaction::from(
        TransactionV1Builder::new_transfer(amount, None, to, transfer_id)
            .unwrap()
            .with_initiator_addr(PublicKey::from(from))
            .with_pricing_mode(pricing)
            .with_chain_name(chain_name)
            .build()
            .unwrap(),
    );

    txn.sign(from);
    let txn_hash = txn.hash();

    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let exec_info = runner
        .main_reactor()
        .storage()
        .read_execution_info(txn_hash)
        .expect("Expected transaction to be included in a block.");

    (
        txn_hash,
        exec_info.block_height,
        exec_info
            .execution_result
            .expect("Exec result should have been stored."),
    )
}

async fn send_wasm_transaction(
    fixture: &mut TestFixture,
    from: &SecretKey,
    pricing: PricingMode,
) -> (TransactionHash, u64, ExecutionResult) {
    let chain_name = fixture.chainspec.network_config.name.clone();

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            TransactionSessionKind::Standard,
            Bytes::from(vec![1]),
            "call",
        )
        .with_chain_name(chain_name)
        .with_pricing_mode(pricing)
        .with_initiator_addr(PublicKey::from(from))
        .build()
        .unwrap(),
    );

    txn.sign(from);
    let txn_hash = txn.hash();

    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let exec_info = runner
        .main_reactor()
        .storage()
        .read_execution_info(txn_hash)
        .expect("Expected transaction to be included in a block.");

    (
        txn_hash,
        exec_info.block_height,
        exec_info
            .execution_result
            .expect("Exec result should have been stored."),
    )
}

fn get_balance(
    fixture: &mut TestFixture,
    account_key: &PublicKey,
    block_height: Option<u64>,
    get_total: bool,
) -> BalanceResult {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let protocol_version = fixture.chainspec.protocol_version();
    let block_height = block_height.unwrap_or(
        runner
            .main_reactor()
            .storage()
            .highest_complete_block_height()
            .expect("missing highest completed block"),
    );
    let block_header = runner
        .main_reactor()
        .storage()
        .read_block_header_by_height(block_height, true)
        .expect("failure to read block header")
        .unwrap();
    let state_hash = *block_header.state_root_hash();
    let balance_handling = if get_total {
        BalanceHandling::Total
    } else {
        let block_time = block_header.timestamp().into();
        BalanceHandling::Available {
            holds_epoch: HoldsEpoch::from_block_time(
                block_time,
                fixture.chainspec.core_config.balance_hold_interval,
            ),
        }
    };
    runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .balance(BalanceRequest::from_public_key(
            state_hash,
            protocol_version,
            account_key.clone(),
            balance_handling,
        ))
}

#[allow(unused)]
fn get_entity(fixture: &mut TestFixture, account_key: PublicKey) -> AddressableEntityResult {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let highest_completed_height = runner
        .main_reactor()
        .storage()
        .highest_complete_block_height()
        .expect("missing highest completed block");
    let state_hash = *runner
        .main_reactor()
        .storage()
        .read_block_header_by_height(highest_completed_height, true)
        .expect("failure to read block header")
        .unwrap()
        .state_root_hash();
    runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .addressable_entity(AddressableEntityRequest::new(
            state_hash,
            AccountHash::from_public_key(&account_key, crypto::blake2b).into(),
        ))
}

fn get_total_supply(fixture: &mut TestFixture, block_height: Option<u64>) -> U512 {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let protocol_version = fixture.chainspec.protocol_version();
    let height = block_height.unwrap_or(
        runner
            .main_reactor()
            .storage()
            .highest_complete_block_height()
            .expect("missing highest completed block"),
    );
    let state_hash = *runner
        .main_reactor()
        .storage()
        .read_block_header_by_height(height, true)
        .expect("failure to read block header")
        .unwrap()
        .state_root_hash();

    let total_supply_req = TotalSupplyRequest::new(state_hash, protocol_version);
    let result = runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .total_supply(total_supply_req);

    if let TotalSupplyResult::Success { total_supply } = result {
        total_supply
    } else {
        panic!("Can't get total supply")
    }
}

fn assert_exec_result_cost(
    exec_result: ExecutionResult,
    expected_cost: U512,
    expected_consumed_gas: Gas,
) {
    match exec_result {
        ExecutionResult::V2(exec_result_v2) => {
            assert_eq!(exec_result_v2.cost, expected_cost);
            assert_eq!(exec_result_v2.consumed, expected_consumed_gas);
        }
        _ => {
            panic!("Unexpected exec result version.")
        }
    }
}

// Returns `true` is the execution result is a success.
pub fn exec_result_is_success(exec_result: &ExecutionResult) -> bool {
    match exec_result {
        ExecutionResult::V2(execution_result_v2) => execution_result_v2.error_message.is_none(),
        ExecutionResult::V1(ExecutionResultV1::Success { .. }) => true,
        ExecutionResult::V1(ExecutionResultV1::Failure { .. }) => false,
    }
}

#[tokio::test]
async fn transfer_cost_fixed_price_no_fee_no_refund() {
    const TRANSFER_AMOUNT: u64 = 30_000_000_000;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = ConfigsOverride::default()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_balance_hold_interval(TimeDiff::from_seconds(5));

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .motes()
        .expect("Expected Alice to have a balance.");

    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        TRANSFER_AMOUNT,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: 1,
        },
        Some(0xDEADBEEF),
    )
    .await;

    let expected_transfer_gas = fixture
        .chainspec
        .system_costs_config
        .mint_costs()
        .transfer
        .into();
    let expected_transfer_cost = expected_transfer_gas; // since we set gas_price_tolerance to 1.

    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost,
        Gas::new(expected_transfer_gas),
    );

    let alice_available_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), true);

    // since FeeHandling is set to NoFee, we expect that there's a hold on Alice's balance for the
    // cost of the transfer. The total balance of Alice now should be the initial balance - the
    // amount transfered to Charlie.
    let alice_expected_total_balance = alice_initial_balance - TRANSFER_AMOUNT;
    // The available balance is the initial balance - the amount transferred to Charlie - the hold
    // for the transfer cost.
    let alice_expected_available_balance = alice_expected_total_balance - expected_transfer_cost;

    assert_eq!(
        alice_total_balance
            .motes()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_total_balance
    );
    assert_eq!(
        alice_available_balance
            .motes()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    let charlie_balance = get_balance(&mut fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_balance
            .motes()
            .expect("Expected Charlie to have a balance")
            .clone(),
        TRANSFER_AMOUNT.into()
    );

    // Check if the hold is released.
    let hold_release_block_height = block_height + 8; // Block time is 1s.
    fixture
        .run_until_block_height(hold_release_block_height, ONE_MIN)
        .await;

    let alice_available_balance = get_balance(
        &mut fixture,
        &alice_public_key,
        Some(hold_release_block_height),
        false,
    );
    let alice_total_balance = get_balance(
        &mut fixture,
        &alice_public_key,
        Some(hold_release_block_height),
        true,
    );

    assert_eq!(alice_available_balance.motes(), alice_total_balance.motes());
}

#[tokio::test]
async fn should_accept_transfer_without_id() {
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = ConfigsOverride::default().with_pricing_handling(PricingHandling::Fixed);
    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;
    let transfer_amount = fixture
        .chainspec
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let (_, _, result) = transfer_to_account(
        &mut fixture,
        transfer_amount,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: 1,
        },
        None,
    )
    .await;

    assert!(exec_result_is_success(&result))
}

#[tokio::test]
async fn failed_transfer_cost_fixed_price_no_fee_no_refund() {
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = ConfigsOverride::default()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_balance_hold_interval(TimeDiff::from_seconds(5));

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let transfer_amount = fixture
        .chainspec
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    // Transfer some token to Charlie.
    let (_txn_hash, _block, exec_result) = transfer_to_account(
        &mut fixture,
        transfer_amount,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: 1,
        },
        None,
    )
    .await;
    assert!(exec_result_is_success(&exec_result));

    // Attempt to transfer more than Charlie has to Bob.
    let bob_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        transfer_amount + 100,
        &charlie_secret_key,
        PublicKey::from(&*bob_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: 1,
        },
        None,
    )
    .await;
    assert!(!exec_result_is_success(&exec_result)); // transaction should have failed.

    let expected_transfer_gas = fixture
        .chainspec
        .system_costs_config
        .mint_costs()
        .transfer
        .into();
    let expected_transfer_cost = expected_transfer_gas; // since we set gas_price_tolerance to 1.

    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost,
        Gas::new(expected_transfer_gas),
    );

    // Even though the transaction failed, a hold must still be in place for the transfer cost.
    let charlie_available_balance =
        get_balance(&mut fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_available_balance
            .motes()
            .expect("Expected Charlie to have a balance")
            .clone(),
        U512::from(transfer_amount) - expected_transfer_cost
    );
}

#[tokio::test]
async fn transfer_cost_classic_price_no_fee_no_refund() {
    const TRANSFER_AMOUNT: u64 = 30_000_000_000;
    const MIN_GAS_PRICE: u8 = 5;
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE * 2;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = ConfigsOverride::default()
        .with_pricing_handling(PricingHandling::Classic)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_balance_hold_interval(TimeDiff::from_seconds(5))
        .with_min_gas_price(MIN_GAS_PRICE)
        .with_max_gas_price(MAX_GAS_PRICE);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .motes()
        .expect("Expected Alice to have a balance.");

    const TRANSFER_GAS: u64 = 100;

    // This transaction should be included since the tolerance is above the min gas price.
    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        TRANSFER_AMOUNT,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Classic {
            payment_amount: TRANSFER_GAS,
            gas_price_tolerance: MIN_GAS_PRICE + 1,
            standard_payment: true,
        },
        None,
    )
    .await;

    let expected_transfer_cost = TRANSFER_GAS * MIN_GAS_PRICE as u64;

    assert!(exec_result_is_success(&exec_result)); // transaction should have succeeded.
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        Gas::new(TRANSFER_GAS),
    );

    let alice_available_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), true);

    // since FeeHandling is set to NoFee, we expect that there's a hold on Alice's balance for the
    // cost of the transfer. The total balance of Alice now should be the initial balance - the
    // amount transfered to Charlie.
    let alice_expected_total_balance = alice_initial_balance - TRANSFER_AMOUNT;
    // The available balance is the initial balance - the amount transferred to Charlie - the hold
    // for the transfer cost.
    let alice_expected_available_balance = alice_expected_total_balance - expected_transfer_cost;

    assert_eq!(
        alice_total_balance
            .motes()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_total_balance
    );
    assert_eq!(
        alice_available_balance
            .motes()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    let charlie_balance = get_balance(&mut fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_balance
            .motes()
            .expect("Expected Charlie to have a balance")
            .clone(),
        TRANSFER_AMOUNT.into()
    );

    // Check if the hold is released.
    let hold_release_block_height = block_height + 8; // Block time is 1s.
    fixture
        .run_until_block_height(hold_release_block_height, ONE_MIN)
        .await;

    let alice_available_balance = get_balance(
        &mut fixture,
        &alice_public_key,
        Some(hold_release_block_height),
        false,
    );
    let alice_total_balance = get_balance(
        &mut fixture,
        &alice_public_key,
        Some(hold_release_block_height),
        true,
    );

    assert_eq!(alice_available_balance.motes(), alice_total_balance.motes());
}

#[tokio::test]
#[should_panic = "within 10 seconds"]
async fn transaction_with_low_threshold_should_not_get_included() {
    const TRANSFER_AMOUNT: u64 = 30_000_000_000;
    const MIN_GAS_PRICE: u8 = 5;
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE * 2;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = ConfigsOverride::default()
        .with_pricing_handling(PricingHandling::Classic)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_balance_hold_interval(TimeDiff::from_seconds(5))
        .with_min_gas_price(MIN_GAS_PRICE)
        .with_max_gas_price(MAX_GAS_PRICE);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    // This transaction should NOT be included since the tolerance is below the min gas price.
    let (_, _, _) = transfer_to_account(
        &mut fixture,
        TRANSFER_AMOUNT,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Classic {
            payment_amount: 1000,
            gas_price_tolerance: MIN_GAS_PRICE - 1,
            standard_payment: true,
        },
        None,
    )
    .await;
}

#[tokio::test]
async fn transfer_fee_is_burnt_no_refund() {
    const MIN_GAS_PRICE: u8 = 5;
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    // make the era longer so that the transaction doesn't land in the switch block.
    let minimum_era_height = 5;
    // make the hold interval very short so we can see the behavior.
    let balance_hold_interval = TimeDiff::from_seconds(5);

    let config = ConfigsOverride::default()
        .with_minimum_era_height(minimum_era_height)
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::Burn)
        .with_balance_hold_interval(balance_hold_interval)
        .with_min_gas_price(MIN_GAS_PRICE)
        .with_max_gas_price(MAX_GAS_PRICE);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    info!("waiting for all nodes to complete era 0");
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let initial_total_supply = get_total_supply(&mut fixture, None);

    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .motes()
        .expect("expected alice to have a balance");

    let transfer_amount = fixture
        .chainspec
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    info!("transferring from alice to charlie");
    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        transfer_amount,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
        None,
    )
    .await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);
    info!("transfer was successful");

    let expected_transfer_gas: u64 = fixture
        .chainspec
        .system_costs_config
        .mint_costs()
        .transfer
        .into();
    let expected_transfer_cost = expected_transfer_gas * MIN_GAS_PRICE as u64;
    info!("checking expected cost");
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
    );

    // The fees should have been burnt so expect the total supply to have been
    // reduced by the fee that was burnt.
    info!("checking total supply");
    let total_supply_after_transaction = get_total_supply(&mut fixture, Some(block_height));
    assert_ne!(
        total_supply_after_transaction, initial_total_supply,
        "total supply should be lowered"
    );
    let diff = initial_total_supply - total_supply_after_transaction;
    assert_eq!(
        diff,
        U512::from(expected_transfer_cost),
        "total supply should be lowered by expected transfer cost"
    );

    let alice_available_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), true);
    let alice_expected_total_balance =
        alice_initial_balance - transfer_amount - expected_transfer_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    info!("checking charlie balance");
    let charlie_balance = get_balance(&mut fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_balance
            .motes()
            .expect("Expected Charlie to have a balance")
            .clone(),
        transfer_amount.into()
    );

    info!("checking alice available balance");
    assert_eq!(
        alice_available_balance
            .motes()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    info!("checking alice total balance");
    assert_eq!(
        alice_total_balance
            .motes()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn fee_is_payed_to_proposer_no_refund() {
    const MIN_GAS_PRICE: u8 = 5;
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]); // Node 0 is effectively guaranteed to be the proposer.

    let config = ConfigsOverride::default()
        .with_minimum_era_height(5) // make the era longer so that the transaction doesn't land in the switch block.
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_balance_hold_interval(TimeDiff::from_seconds(5))
        .with_min_gas_price(MIN_GAS_PRICE)
        .with_max_gas_price(MAX_GAS_PRICE);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let bob_public_key = PublicKey::from(&*bob_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let bob_initial_balance = *get_balance(&mut fixture, &bob_public_key, None, true)
        .motes()
        .expect("Expected Bob to have a balance.");
    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .motes()
        .expect("Expected Alice to have a balance.");

    let transfer_amount = fixture
        .chainspec
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        transfer_amount,
        &bob_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
        None,
    )
    .await;

    assert!(exec_result_is_success(&exec_result)); // transaction should have succeeded.

    let expected_transfer_gas: u64 = fixture
        .chainspec
        .system_costs_config
        .mint_costs()
        .transfer
        .into();
    let expected_transfer_cost = expected_transfer_gas * MIN_GAS_PRICE as u64;
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
    );

    let bob_available_balance =
        get_balance(&mut fixture, &bob_public_key, Some(block_height), false);
    let bob_total_balance = get_balance(&mut fixture, &bob_public_key, Some(block_height), true);

    let alice_available_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), true);

    // since Alice was the proposer of the block, it should get back the transfer fee since
    // FeeHandling is set to PayToProposer.
    let bob_expected_total_balance = bob_initial_balance - transfer_amount - expected_transfer_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    let alice_expected_total_balance = alice_initial_balance + expected_transfer_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    let charlie_balance = get_balance(&mut fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_balance
            .motes()
            .expect("Expected Charlie to have a balance")
            .clone(),
        transfer_amount.into()
    );

    assert_eq!(
        bob_available_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        bob_expected_available_balance
    );

    assert_eq!(
        bob_total_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        bob_expected_total_balance
    );

    assert_eq!(
        alice_available_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    assert_eq!(
        alice_total_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn native_operations_fees_are_not_refunded() {
    const MIN_GAS_PRICE: u8 = 5;
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]); // Node 0 is effectively guaranteed to be the proposer.

    let config = ConfigsOverride::default()
        .with_minimum_era_height(5) // make the era longer so that the transaction doesn't land in the switch block.
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(1, 2),
        })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_balance_hold_interval(TimeDiff::from_seconds(5))
        .with_min_gas_price(MIN_GAS_PRICE)
        .with_max_gas_price(MAX_GAS_PRICE);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let bob_public_key = PublicKey::from(&*bob_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let bob_initial_balance = *get_balance(&mut fixture, &bob_public_key, None, true)
        .motes()
        .expect("Expected Bob to have a balance.");
    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .motes()
        .expect("Expected Alice to have a balance.");

    let transfer_amount = fixture
        .chainspec
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        transfer_amount,
        &bob_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
        None,
    )
    .await;

    assert!(exec_result_is_success(&exec_result)); // transaction should have succeeded.

    let expected_transfer_gas: u64 = fixture
        .chainspec
        .system_costs_config
        .mint_costs()
        .transfer
        .into();
    let expected_transfer_cost = expected_transfer_gas * MIN_GAS_PRICE as u64;
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
    );

    let bob_available_balance =
        get_balance(&mut fixture, &bob_public_key, Some(block_height), false);
    let bob_total_balance = get_balance(&mut fixture, &bob_public_key, Some(block_height), true);

    let alice_available_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), true);

    // Bob shouldn't get a refund since there is no refund for native transfers.
    let bob_expected_total_balance = bob_initial_balance - transfer_amount - expected_transfer_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the full fee since there is no refund for native transfers.
    let alice_expected_total_balance = alice_initial_balance + expected_transfer_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    let charlie_balance = get_balance(&mut fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_balance
            .motes()
            .expect("Expected Charlie to have a balance")
            .clone(),
        transfer_amount.into()
    );

    assert_eq!(
        bob_available_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        bob_expected_available_balance
    );

    assert_eq!(
        bob_total_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        bob_expected_total_balance
    );

    assert_eq!(
        alice_available_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    assert_eq!(
        alice_total_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn wasm_transaction_fees_are_refunded() {
    const MIN_GAS_PRICE: u8 = 5;
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]); // Node 0 is effectively guaranteed to be the proposer.

    let refund_ratio = Ratio::new(1, 2);
    let config = ConfigsOverride::default()
        .with_minimum_era_height(5) // make the era longer so that the transaction doesn't land in the switch block.
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_balance_hold_interval(TimeDiff::from_seconds(5))
        .with_min_gas_price(MIN_GAS_PRICE)
        .with_max_gas_price(MAX_GAS_PRICE);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let bob_public_key = PublicKey::from(&*bob_secret_key);

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let bob_initial_balance = *get_balance(&mut fixture, &bob_public_key, None, true)
        .motes()
        .expect("Expected Bob to have a balance.");
    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .motes()
        .expect("Expected Alice to have a balance.");

    let (_txn_hash, block_height, exec_result) = send_wasm_transaction(
        &mut fixture,
        &bob_secret_key,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
    )
    .await;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.

    let expected_transaction_gas: u64 = fixture
        .chainspec
        .system_costs_config
        .standard_transaction_limit();
    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0), /* expect that this transaction doesn't consume any gas since it has
                      * invalid wasm. */
    );

    let bob_available_balance =
        get_balance(&mut fixture, &bob_public_key, Some(block_height), false);
    let bob_total_balance = get_balance(&mut fixture, &bob_public_key, Some(block_height), true);

    let alice_available_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), true);

    // Bob should get back half of the cost for the unspent gas. Since this transaction consumed 0
    // gas, the unspent gas is equal to the limit.
    let refund_amount: U512 = (refund_ratio * Ratio::from(expected_transaction_cost))
        .to_integer()
        .into();

    let bob_expected_total_balance =
        bob_initial_balance - expected_transaction_cost + refund_amount;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the non-refunded part of the fee since it's set to pay to proposer
    let alice_expected_total_balance =
        alice_initial_balance + expected_transaction_cost - refund_amount;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_available_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        bob_expected_available_balance
    );

    assert_eq!(
        bob_total_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        bob_expected_total_balance
    );

    assert_eq!(
        alice_available_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    assert_eq!(
        alice_total_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn wasm_transaction_refunds_are_burnt() {
    const MIN_GAS_PRICE: u8 = 5;
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]); // Node 0 is effectively guaranteed to be the proposer.

    let refund_ratio = Ratio::new(1, 2);
    let config = ConfigsOverride::default()
        .with_minimum_era_height(5) // make the era longer so that the transaction doesn't land in the switch block.
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_balance_hold_interval(TimeDiff::from_seconds(5))
        .with_min_gas_price(MIN_GAS_PRICE)
        .with_max_gas_price(MAX_GAS_PRICE);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let bob_public_key = PublicKey::from(&*bob_secret_key);

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let bob_initial_balance = *get_balance(&mut fixture, &bob_public_key, None, true)
        .motes()
        .expect("Expected Bob to have a balance.");
    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .motes()
        .expect("Expected Alice to have a balance.");
    let initial_total_supply = get_total_supply(&mut fixture, None);

    let (_txn_hash, block_height, exec_result) = send_wasm_transaction(
        &mut fixture,
        &bob_secret_key,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
    )
    .await;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    let expected_transaction_gas: u64 = fixture
        .chainspec
        .system_costs_config
        .standard_transaction_limit();
    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0), /* expect that this transaction doesn't consume any gas since it has
                      * invalid wasm. */
    );

    // Bob should get back half of the cost for the unspent gas. Since this transaction consumed 0
    // gas, the unspent gas is equal to the limit.
    let refund_amount: U512 = (refund_ratio * Ratio::from(expected_transaction_cost))
        .to_integer()
        .into();

    // The refund should have been burnt. So expect the total supply should have been reduced by the
    // refund amount that was burnt.
    let total_supply_after_transaction = get_total_supply(&mut fixture, Some(block_height));
    assert_eq!(
        total_supply_after_transaction,
        initial_total_supply - refund_amount
    );

    let bob_available_balance =
        get_balance(&mut fixture, &bob_public_key, Some(block_height), false);
    let bob_total_balance = get_balance(&mut fixture, &bob_public_key, Some(block_height), true);

    let alice_available_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance =
        get_balance(&mut fixture, &alice_public_key, Some(block_height), true);

    // Bob doesn't get a refund. The refund is burnt.
    let bob_expected_total_balance = bob_initial_balance - expected_transaction_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the non-refunded part of the fee since it's set to pay to proposer
    let alice_expected_total_balance =
        alice_initial_balance + expected_transaction_cost - refund_amount;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_available_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        bob_expected_available_balance
    );

    assert_eq!(
        bob_total_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        bob_expected_total_balance
    );

    assert_eq!(
        alice_available_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    assert_eq!(
        alice_total_balance
            .motes()
            .expect("Expected Bob to have a balance")
            .clone(),
        alice_expected_total_balance
    );
}
