use super::*;
use casper_storage::data_access_layer::{BalanceIdentifier, ProofHandling};
use casper_types::GasLimited;
use once_cell::sync::Lazy;

use casper_types::{bytesrepr::Bytes, execution::ExecutionResultV1, TransactionSessionKind};

static ALICE_SECRET_KEY: Lazy<Arc<SecretKey>> = Lazy::new(|| {
    Arc::new(SecretKey::ed25519_from_bytes([0xAA; SecretKey::ED25519_LENGTH]).unwrap())
});
static ALICE_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ALICE_SECRET_KEY.clone()));

static BOB_SECRET_KEY: Lazy<Arc<SecretKey>> = Lazy::new(|| {
    Arc::new(SecretKey::ed25519_from_bytes([0xBB; SecretKey::ED25519_LENGTH]).unwrap())
});
static BOB_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&*BOB_SECRET_KEY.clone()));

static CHARLIE_SECRET_KEY: Lazy<Arc<SecretKey>> = Lazy::new(|| {
    Arc::new(SecretKey::ed25519_from_bytes([0xCC; SecretKey::ED25519_LENGTH]).unwrap())
});
static CHARLIE_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*CHARLIE_SECRET_KEY.clone()));

const MIN_GAS_PRICE: u8 = 5;
const CHAIN_NAME: &str = "single-transaction-test-net";

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

    info!("transfer_to_account starting run_until_executed_transaction");
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    info!("transfer_to_account finished run_until_executed_transaction");
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
        BalanceHandling::Available
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
            ProofHandling::NoProofs,
        ))
}

fn get_bids(fixture: &mut TestFixture, block_height: Option<u64>) -> Option<Vec<BidKind>> {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
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

    runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .bids(BidsRequest::new(state_hash))
        .into_option()
}

fn get_payment_purse_balance(
    fixture: &mut TestFixture,
    block_height: Option<u64>,
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
    runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .balance(BalanceRequest::new(
            state_hash,
            protocol_version,
            BalanceIdentifier::Payment,
            BalanceHandling::Available,
            ProofHandling::NoProofs,
        ))
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

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .available_balance()
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
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_total_balance
    );
    assert_eq!(
        alice_available_balance
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    let charlie_balance = get_balance(&mut fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_balance
            .available_balance()
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

    assert_eq!(
        alice_available_balance.available_balance(),
        alice_total_balance.available_balance()
    );
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
            .available_balance()
            .expect("Expected Charlie to have a balance")
            .clone(),
        U512::from(transfer_amount) - expected_transfer_cost
    );
}

#[tokio::test]
async fn transfer_cost_classic_price_no_fee_no_refund() {
    const TRANSFER_AMOUNT: u64 = 30_000_000_000;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Classic)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .available_balance()
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
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_total_balance
    );
    assert_eq!(
        alice_available_balance
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    let charlie_balance = get_balance(&mut fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_balance
            .available_balance()
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

    assert_eq!(
        alice_available_balance.available_balance(),
        alice_total_balance.available_balance()
    );
}

#[tokio::test]
#[should_panic = "within 10 seconds"]
async fn transaction_with_low_threshold_should_not_get_included() {
    const TRANSFER_AMOUNT: u64 = 30_000_000_000;
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE * 2;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Classic)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));

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
async fn native_operations_fees_are_not_refunded() {
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]); // Node 0 is effectively guaranteed to be the proposer.

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(1, 2),
        })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let bob_public_key = PublicKey::from(&*bob_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let bob_initial_balance = *get_balance(&mut fixture, &bob_public_key, None, true)
        .total_balance()
        .expect("Expected Bob to have a balance.");
    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .total_balance()
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
        *get_balance(&mut fixture, &bob_public_key, Some(block_height), false)
            .available_balance()
            .expect("Expected Bob to have a balance");
    let bob_total_balance = *get_balance(&mut fixture, &bob_public_key, Some(block_height), true)
        .total_balance()
        .expect("Expected Bob to have a balance");

    let alice_available_balance =
        *get_balance(&mut fixture, &alice_public_key, Some(block_height), false)
            .available_balance()
            .expect("Expected Alice to have a balance");
    let alice_total_balance =
        *get_balance(&mut fixture, &alice_public_key, Some(block_height), true)
            .total_balance()
            .expect("Expected Alice to have a balance");

    // Bob shouldn't get a refund since there is no refund for native transfers.
    let bob_expected_total_balance = bob_initial_balance - transfer_amount - expected_transfer_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the full fee since there is no refund for native transfers.
    let alice_expected_total_balance = alice_initial_balance + expected_transfer_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    let charlie_balance =
        *get_balance(&mut fixture, &charlie_public_key, Some(block_height), false)
            .available_balance()
            .expect("Expected Charlie to have a balance");
    assert_eq!(charlie_balance.clone(), transfer_amount.into());

    assert_eq!(
        bob_available_balance.clone(),
        bob_expected_available_balance
    );

    assert_eq!(bob_total_balance.clone(), bob_expected_total_balance);

    assert_eq!(
        alice_available_balance.clone(),
        alice_expected_available_balance
    );

    assert_eq!(alice_total_balance.clone(), alice_expected_total_balance);
}

#[tokio::test]
async fn wasm_transaction_fees_are_refunded() {
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]); // Node 0 is effectively guaranteed to be the proposer.

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let bob_public_key = PublicKey::from(&*bob_secret_key);

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let bob_initial_balance = *get_balance(&mut fixture, &bob_public_key, None, true)
        .total_balance()
        .expect("Expected Bob to have a balance.");
    let alice_initial_balance = *get_balance(&mut fixture, &alice_public_key, None, true)
        .total_balance()
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
        *get_balance(&mut fixture, &bob_public_key, Some(block_height), false)
            .available_balance()
            .expect("Expected Bob to have a balance");
    let bob_total_balance = *get_balance(&mut fixture, &bob_public_key, Some(block_height), true)
        .total_balance()
        .expect("Expected Bob to have a balance");

    let alice_available_balance =
        *get_balance(&mut fixture, &alice_public_key, Some(block_height), false)
            .available_balance()
            .expect("Expected Alice to have a balance");
    let alice_total_balance =
        *get_balance(&mut fixture, &alice_public_key, Some(block_height), true)
            .total_balance()
            .expect("Expected Alice to have a balance");

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
        bob_available_balance.clone(),
        bob_expected_available_balance
    );

    assert_eq!(bob_total_balance.clone(), bob_expected_total_balance);

    assert_eq!(
        alice_available_balance.clone(),
        alice_expected_available_balance
    );

    assert_eq!(alice_total_balance.clone(), alice_expected_total_balance);
}

struct SingleTransactionTestCase {
    fixture: TestFixture,
    alice_public_key: PublicKey,
    bob_public_key: PublicKey,
    charlie_public_key: PublicKey,
}

#[derive(Debug, PartialEq)]
struct BalanceAmount {
    available: U512,
    total: U512,
}

impl SingleTransactionTestCase {
    fn default_test_config() -> ConfigsOverride {
        ConfigsOverride::default()
            .with_minimum_era_height(5) // make the era longer so that the transaction doesn't land in the switch block.
            .with_balance_hold_interval(TimeDiff::from_seconds(5))
            .with_min_gas_price(MIN_GAS_PRICE)
            .with_max_gas_price(MIN_GAS_PRICE)
            .with_chain_name("single-transaction-test-net".to_string())
    }

    async fn new(
        alice_secret_key: Arc<SecretKey>,
        bob_secret_key: Arc<SecretKey>,
        charlie_secret_key: Arc<SecretKey>,
        network_config: Option<ConfigsOverride>,
    ) -> Self {
        let rng = TestRng::new();

        let alice_public_key = PublicKey::from(&*alice_secret_key);
        let bob_public_key = PublicKey::from(&*bob_secret_key);
        let charlie_public_key = PublicKey::from(&*charlie_secret_key);

        let stakes = vec![
            (alice_public_key.clone(), U512::from(u128::MAX)), /* Node 0 is effectively
                                                                * guaranteed to be the
                                                                * proposer. */
            (bob_public_key.clone(), U512::from(1)),
        ]
        .into_iter()
        .collect();

        let fixture = TestFixture::new_with_keys(
            rng,
            vec![alice_secret_key.clone(), bob_secret_key.clone()],
            stakes,
            network_config,
        )
        .await;
        Self {
            fixture,
            alice_public_key,
            bob_public_key,
            charlie_public_key,
        }
    }

    fn chainspec(&self) -> &Chainspec {
        &self.fixture.chainspec
    }

    fn get_balances(
        &mut self,
        block_height: Option<u64>,
    ) -> (BalanceAmount, BalanceAmount, Option<BalanceAmount>) {
        let alice_total_balance = *get_balance(
            &mut self.fixture,
            &self.alice_public_key,
            block_height,
            true,
        )
        .total_balance()
        .expect("Expected Alice to have a balance.");
        let bob_total_balance =
            *get_balance(&mut self.fixture, &self.bob_public_key, block_height, true)
                .total_balance()
                .expect("Expected Bob to have a balance.");

        let alice_available_balance = *get_balance(
            &mut self.fixture,
            &self.alice_public_key,
            block_height,
            false,
        )
        .available_balance()
        .expect("Expected Alice to have a balance.");
        let bob_available_balance =
            *get_balance(&mut self.fixture, &self.bob_public_key, block_height, false)
                .available_balance()
                .expect("Expected Bob to have a balance.");

        let charlie_available_balance = get_balance(
            &mut self.fixture,
            &self.charlie_public_key,
            block_height,
            false,
        )
        .available_balance()
        .copied();

        let charlie_total_balance = get_balance(
            &mut self.fixture,
            &self.charlie_public_key,
            block_height,
            true,
        )
        .available_balance()
        .copied();

        let charlie_amount = charlie_available_balance.map(|avail_balance| BalanceAmount {
            available: avail_balance,
            total: charlie_total_balance.unwrap(),
        });

        (
            BalanceAmount {
                available: alice_available_balance,
                total: alice_total_balance,
            },
            BalanceAmount {
                available: bob_available_balance,
                total: bob_total_balance,
            },
            charlie_amount,
        )
    }

    async fn send_transaction(
        &mut self,
        txn: Transaction,
    ) -> (TransactionHash, u64, ExecutionResult) {
        let txn_hash = txn.hash();

        self.fixture.inject_transaction(txn).await;
        self.fixture
            .run_until_executed_transaction(&txn_hash, TEN_SECS)
            .await;

        let (_node_id, runner) = self.fixture.network.nodes().iter().next().unwrap();
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

    fn get_total_supply(&mut self, block_height: Option<u64>) -> U512 {
        let (_node_id, runner) = self.fixture.network.nodes().iter().next().unwrap();
        let protocol_version = self.fixture.chainspec.protocol_version();
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

    fn get_accumulate_purse_balance(
        &mut self,
        block_height: Option<u64>,
        get_total: bool,
    ) -> BalanceResult {
        let (_node_id, runner) = self.fixture.network.nodes().iter().next().unwrap();
        let protocol_version = self.fixture.chainspec.protocol_version();
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
            BalanceHandling::Available
        };
        runner
            .main_reactor()
            .contract_runtime()
            .data_access_layer()
            .balance(BalanceRequest::new(
                state_hash,
                protocol_version,
                BalanceIdentifier::Accumulate,
                balance_handling,
                ProofHandling::NoProofs,
            ))
    }
}

async fn wasm_transaction_refunds_are_burnt(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    let expected_transaction_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .standard_transaction_limit(),
    );
    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
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
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - refund_amount
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob doesn't get a refund. The refund is burnt.
    let bob_expected_total_balance = bob_initial_balance.total - expected_transaction_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the non-refunded part of the fee since it's set to pay to proposer
    let alice_expected_total_balance =
        alice_initial_balance.total + expected_transaction_cost - refund_amount;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn wasm_transaction_refunds_are_burnt_fixed_pricing() {
    wasm_transaction_refunds_are_burnt(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn wasm_transaction_refunds_are_burnt_classic_pricing() {
    wasm_transaction_refunds_are_burnt(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn only_refunds_are_burnt_no_fee(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;
    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    // Fixed transaction pricing.
    let expected_transaction_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .standard_transaction_limit(),
    );
    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0), /* expect that this transaction doesn't consume any gas since it has
                      * invalid wasm. */
    );

    // This transaction consumed 0 gas, the unspent gas is equal to the limit, so we apply the
    // refund ratio to the full transaction cost.
    let refund_amount: U512 = (refund_ratio * Ratio::from(expected_transaction_cost))
        .to_integer()
        .into();

    // We set it up so that the refunds are burnt so check this.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - refund_amount
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob doesn't get a refund. The refund is burnt. A hold is put in in place for the
    // transaction cost.
    let bob_balance_hold = U512::from(expected_transaction_cost) - refund_amount;
    let bob_expected_total_balance = bob_initial_balance.total - refund_amount;
    let bob_expected_available_balance = bob_current_balance.total - bob_balance_hold;

    // Alice should't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn only_refunds_are_burnt_no_fee_fixed_pricing() {
    only_refunds_are_burnt_no_fee(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn only_refunds_are_burnt_no_fee_classic_pricing() {
    only_refunds_are_burnt_no_fee(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn fees_and_refunds_are_burnt_separately(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::Burn);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;
    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    // Fixed transaction pricing.
    let expected_transaction_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .standard_transaction_limit(),
    );
    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0), /* expect that this transaction doesn't consume any gas since it has
                      * invalid wasm. */
    );

    // Both refunds and fees should be burnt (even though they are burnt separately). Refund + fee
    // amounts to the txn cost so expect that the total supply is reduced by that amount.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - expected_transaction_cost
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // The refund and the fees are burnt. No holds should be in place.
    let bob_expected_total_balance = bob_initial_balance.total - expected_transaction_cost;
    let bob_expected_available_balance = bob_current_balance.total;

    // Alice should't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn fees_and_refunds_are_burnt_separately_fixed_pricing() {
    fees_and_refunds_are_burnt_separately(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn fees_and_refunds_are_burnt_separately_classic_pricing() {
    fees_and_refunds_are_burnt_separately(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn refunds_are_payed_and_fees_are_burnt(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::Burn);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    // Fixed transaction pricing.
    let expected_transaction_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .standard_transaction_limit(),
    );
    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0), /* expect that this transaction doesn't consume any gas since it has
                      * invalid wasm. */
    );

    // This transaction consumed 0 gas, the unspent gas is equal to the limit, so we apply the
    // refund ratio to the full transaction cost.
    let refund_amount: U512 = (refund_ratio * Ratio::from(expected_transaction_cost))
        .to_integer()
        .into();

    // Only fees are burnt, so the refund_amount should still be in the total supply.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - expected_transaction_cost + refund_amount
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob should get back the refund. The fees are burnt and no holds should be in place.
    let bob_expected_total_balance =
        bob_initial_balance.total - expected_transaction_cost + refund_amount;
    let bob_expected_available_balance = bob_current_balance.total;

    // Alice should't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn refunds_are_payed_and_fees_are_burnt_fixed_pricing() {
    refunds_are_payed_and_fees_are_burnt(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn refunds_are_payed_and_fees_are_burnt_classic_pricing() {
    refunds_are_payed_and_fees_are_burnt(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn refunds_are_payed_and_fees_are_on_hold(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, _gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);
    // Fixed transaction pricing.
    let expected_consumed_gas = Gas::new(0); // expect that this transaction doesn't consume any gas since it has invalid wasm.
    let expected_transaction_cost =
        txn.gas_limit(test.chainspec()).unwrap().value() * min_gas_price;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost,
        expected_consumed_gas,
    );

    // This transaction consumed 0 gas, the unspent gas is equal to the limit, so we apply the
    // refund ratio to the full transaction cost.
    let refund_amount: U512 = (Ratio::<U512>::new(
        (*refund_ratio.numer()).into(),
        (*refund_ratio.denom()).into(),
    ) * Ratio::from(expected_transaction_cost))
    .to_integer();

    // Nothing is burnt so total supply should be the same.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob should get back the refund. The fees should be on hold, so Bob's total should be the
    // same as initial.
    let bob_expected_total_balance = bob_initial_balance.total;
    let bob_expected_available_balance =
        bob_current_balance.total - expected_transaction_cost + refund_amount;

    // Alice should't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn refunds_are_payed_and_fees_are_on_hold_fixed_pricing() {
    refunds_are_payed_and_fees_are_on_hold(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn refunds_are_payed_and_fees_are_on_hold_classic_pricing() {
    refunds_are_payed_and_fees_are_on_hold(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

#[tokio::test]
async fn only_refunds_are_burnt_no_fee_custom_payment() {
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE;

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Classic)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::Burn);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    // This contract uses custom payment.
    let contract_file = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("ee_601_regression.wasm");
    let module_bytes = Bytes::from(std::fs::read(contract_file).expect("cannot read module bytes"));

    let expected_transaction_gas = 1000u64;
    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(TransactionSessionKind::Standard, module_bytes, "call")
            .with_chain_name(CHAIN_NAME)
            .with_pricing_mode(PricingMode::Classic {
                payment_amount: expected_transaction_gas,
                gas_price_tolerance: MIN_GAS_PRICE,
                standard_payment: false,
            })
            .with_initiator_addr(BOB_PUBLIC_KEY.clone())
            .build()
            .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because we didn't request enough gas for this transaction
                                                    // to succeed.
    match exec_result {
        ExecutionResult::V2(exec_result_v2) => {
            assert_eq!(exec_result_v2.cost, expected_transaction_cost.into());
        }
        _ => {
            panic!("Unexpected exec result version.")
        }
    }

    // This transaction consumed all the gas so there should be no refund.
    let refund_amount = U512::from(0);

    // Expect that the fees are burnt.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - expected_transaction_cost + refund_amount
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob should get a refund. Since the contract doesn't set a custom purse for the refund, it
    // should get the refund in the main purse.
    let bob_expected_total_balance =
        bob_initial_balance.total - expected_transaction_cost + refund_amount;
    let bob_expected_available_balance = bob_expected_total_balance; // No holds expected.

    // Alice should't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn no_refund_no_fee_custom_payment() {
    const MAX_GAS_PRICE: u8 = MIN_GAS_PRICE;

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Classic)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    // This contract uses custom payment.
    let contract_file = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("ee_601_regression.wasm");
    let module_bytes = Bytes::from(std::fs::read(contract_file).expect("cannot read module bytes"));

    let expected_transaction_gas = 1000u64;
    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(TransactionSessionKind::Standard, module_bytes, "call")
            .with_chain_name(CHAIN_NAME)
            .with_pricing_mode(PricingMode::Classic {
                payment_amount: expected_transaction_gas,
                gas_price_tolerance: MIN_GAS_PRICE,
                standard_payment: false,
            })
            .with_initiator_addr(BOB_PUBLIC_KEY.clone())
            .build()
            .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    // expected to fail due to insufficient funding
    assert!(!exec_result_is_success(&exec_result), "should have failed");
    match exec_result {
        ExecutionResult::V2(exec_result_v2) => {
            assert_eq!(exec_result_v2.cost, expected_transaction_cost.into());
        }
        _ => {
            panic!("Unexpected exec result version.")
        }
    }

    let payment_purse_balance = get_payment_purse_balance(&mut test.fixture, Some(block_height));
    assert_eq!(
        *payment_purse_balance
            .total_balance()
            .expect("should have total balance"),
        U512::zero(),
        "payment purse should have a 0 balance"
    );

    // we're not burning anything, so total supply should be the same
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply,
        "total supply should be the same before and after"
    );

    // updated balances
    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));

    // the proposer's balance should be the same because we are in no fee mode
    assert_eq!(
        alice_initial_balance, alice_current_balance,
        "the proposers balance should be unchanged as we are in no fee mode"
    );

    // the initiator should have a hold equal to the cost
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_initial_balance.total,
        "bob's total balance should be unchanged as we are in no fee mode"
    );

    assert_ne!(
        bob_current_balance.available.clone(),
        bob_initial_balance.total,
        "bob's available balance and total balance should not be the same"
    );

    let bob_expected_available_balance = bob_initial_balance.total - expected_transaction_cost;
    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance,
        "bob's available balance should reflect a hold for the cost"
    );
}

async fn transfer_fee_is_burnt_no_refund(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::Burn);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = test
        .chainspec()
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let txn = transfer_txn(
        ALICE_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        txn_pricing_mode,
        transfer_amount,
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, _, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    let expected_transfer_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .mint_costs()
            .transfer
            .into(),
    );
    let expected_transfer_cost = expected_transfer_gas * min_gas_price as u64;

    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
    );

    // The fees should have been burnt so expect the total supply to have been
    // reduced by the fee that was burnt.
    let total_supply_after_txn = test.get_total_supply(Some(block_height));
    assert_ne!(
        total_supply_after_txn, initial_total_supply,
        "total supply should be lowered"
    );
    let diff = initial_total_supply - total_supply_after_txn;
    assert_eq!(
        diff,
        U512::from(expected_transfer_cost),
        "total supply should be lowered by expected transfer cost"
    );

    // Get the current balances after the transaction and check them.
    let (alice_current_balance, _, charlie_balance) = test.get_balances(Some(block_height));
    let alice_expected_total_balance =
        alice_initial_balance.total - transfer_amount - expected_transfer_cost;
    let alice_expected_available_balance = alice_expected_total_balance;
    assert_eq!(
        charlie_balance
            .expect("Charlie should have a balance.")
            .total,
        transfer_amount.into(),
    );
    assert_eq!(
        alice_current_balance.available, alice_expected_available_balance,
        "alice available balance should match"
    );
    assert_eq!(alice_current_balance.total, alice_expected_total_balance);
}

#[tokio::test]
async fn transfer_fee_is_burnt_no_refund_fixed_pricing() {
    transfer_fee_is_burnt_no_refund(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn transfer_fee_is_burnt_no_refund_classic_pricing() {
    transfer_fee_is_burnt_no_refund(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn fee_is_payed_to_proposer_no_refund(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = test
        .chainspec()
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        txn_pricing_mode,
        transfer_amount,
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    let expected_transfer_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .mint_costs()
            .transfer
            .into(),
    );
    let expected_transfer_cost = expected_transfer_gas * min_gas_price as u64;

    assert!(exec_result_is_success(&exec_result));
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
    );

    // Nothing should be burnt.
    assert_eq!(
        initial_total_supply,
        test.get_total_supply(Some(block_height)),
        "total supply should unchanged"
    );

    let (alice_current_balance, bob_current_balance, charlie_balance) =
        test.get_balances(Some(block_height));

    // since Alice was the proposer of the block, it should get back the transfer fee since
    // FeeHandling is set to PayToProposer.
    let bob_expected_total_balance =
        bob_initial_balance.total - transfer_amount - expected_transfer_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    let alice_expected_total_balance = alice_initial_balance.total + expected_transfer_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        charlie_balance
            .expect("Expected Charlie to have a balance")
            .total,
        transfer_amount.into()
    );
    assert_eq!(
        bob_current_balance.available,
        bob_expected_available_balance
    );
    assert_eq!(bob_current_balance.total, bob_expected_total_balance);
    assert_eq!(
        alice_current_balance.available,
        alice_expected_available_balance
    );
    assert_eq!(alice_current_balance.total, alice_expected_total_balance);
}

#[tokio::test]
async fn fee_is_payed_to_proposer_no_refund_fixed_pricing() {
    fee_is_payed_to_proposer_no_refund(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn fee_is_payed_to_proposer_no_refund_classic_pricing() {
    fee_is_payed_to_proposer_no_refund(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn wasm_transaction_fees_are_refunded_to_proposer(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    let expected_transaction_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .standard_transaction_limit(),
    );
    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0), /* expect that this transaction doesn't consume any gas since it has
                      * invalid wasm. */
    );

    // Nothing is burnt so total supply should be the same.
    assert_eq!(
        initial_total_supply,
        test.get_total_supply(Some(block_height))
    );

    // Bob should get back half of the cost for the unspent gas. Since this transaction consumed 0
    // gas, the unspent gas is equal to the limit.
    let refund_amount: U512 = (refund_ratio * Ratio::from(expected_transaction_cost))
        .to_integer()
        .into();

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    let bob_expected_total_balance =
        bob_initial_balance.total - expected_transaction_cost + refund_amount;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the non-refunded part of the fee since it's set to pay to proposer
    let alice_expected_total_balance =
        alice_initial_balance.total + expected_transaction_cost - refund_amount;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn wasm_transaction_fees_are_refunded_to_proposer_fixed_pricing() {
    wasm_transaction_fees_are_refunded_to_proposer(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn wasm_transaction_fees_are_refunded_to_proposer_classic_pricing() {
    wasm_transaction_fees_are_refunded_to_proposer(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn fee_is_accumulated_and_distributed_no_refund(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let admins: BTreeSet<PublicKey> = vec![ALICE_PUBLIC_KEY.clone(), BOB_PUBLIC_KEY.clone()]
        .into_iter()
        .collect();

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::Accumulate)
        .with_administrators(admins);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = test
        .chainspec()
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        txn_pricing_mode,
        transfer_amount,
    );

    let expected_transfer_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .mint_costs()
            .transfer
            .into(),
    );
    let expected_transfer_cost = expected_transfer_gas * min_gas_price as u64;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;
    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let acc_purse_initial_balance = *test
        .get_accumulate_purse_balance(None, false)
        .available_balance()
        .expect("Accumulate purse should have a balance.");

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(exec_result_is_success(&exec_result));
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
    );

    assert_eq!(
        initial_total_supply,
        test.get_total_supply(Some(block_height)),
        "total supply should remain unchanged"
    );

    let (alice_current_balance, bob_current_balance, charlie_balance) =
        test.get_balances(Some(block_height));

    let bob_expected_total_balance =
        bob_initial_balance.total - transfer_amount - expected_transfer_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        charlie_balance
            .expect("Expected Charlie to have a balance")
            .total,
        transfer_amount.into()
    );

    assert_eq!(
        bob_current_balance.available,
        bob_expected_available_balance
    );
    assert_eq!(bob_current_balance.total, bob_expected_total_balance);
    assert_eq!(
        alice_current_balance.available,
        alice_expected_available_balance
    );
    assert_eq!(alice_current_balance.total, alice_expected_total_balance);

    let acc_purse_balance = *test
        .get_accumulate_purse_balance(Some(block_height), false)
        .available_balance()
        .expect("Accumulate purse should have a balance.");

    // The fees should be sent to the accumulation purse.
    assert_eq!(
        acc_purse_balance - acc_purse_initial_balance,
        expected_transfer_cost.into()
    );

    test.fixture
        .run_until_block_height(block_height + 10, ONE_MIN)
        .await;

    let accumulate_purse_balance = *test
        .get_accumulate_purse_balance(Some(block_height + 10), false)
        .available_balance()
        .expect("Accumulate purse should have a balance.");

    assert_eq!(accumulate_purse_balance, U512::from(0));
}

#[tokio::test]
async fn fee_is_accumulated_and_distributed_no_refund_fixed_pricing() {
    fee_is_accumulated_and_distributed_no_refund(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn fee_is_accumulated_and_distributed_no_refund_classic_pricing() {
    fee_is_accumulated_and_distributed_no_refund(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

fn transfer_txn<A: Into<U512>>(
    from: Arc<SecretKey>,
    to: &PublicKey,
    pricing_mode: PricingMode,
    amount: A,
) -> Transaction {
    let mut txn = Transaction::from(
        TransactionV1Builder::new_transfer(amount, None, to.clone(), None)
            .unwrap()
            .with_initiator_addr(PublicKey::from(&*from))
            .with_pricing_mode(pricing_mode)
            .with_chain_name(CHAIN_NAME)
            .build()
            .unwrap(),
    );
    txn.sign(&from);
    txn
}

fn invalid_wasm_txn(initiator: Arc<SecretKey>, pricing_mode: PricingMode) -> Transaction {
    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            TransactionSessionKind::Standard,
            Bytes::from(vec![1]),
            "call",
        )
        .with_chain_name(CHAIN_NAME)
        .with_pricing_mode(pricing_mode)
        .with_initiator_addr(PublicKey::from(&*initiator))
        .build()
        .unwrap(),
    );
    txn.sign(&initiator);
    txn
}

fn match_pricing_mode(txn_pricing_mode: &PricingMode) -> (PricingHandling, u8, Option<u64>) {
    match txn_pricing_mode {
        PricingMode::Classic {
            gas_price_tolerance,
            payment_amount,
            ..
        } => (
            PricingHandling::Classic,
            *gas_price_tolerance,
            Some(*payment_amount),
        ),
        PricingMode::Fixed {
            gas_price_tolerance,
        } => (PricingHandling::Fixed, *gas_price_tolerance, None),
        PricingMode::Reserved { .. } => unimplemented!(),
    }
}

#[tokio::test]
async fn holds_should_be_added_and_cleared_fixed_pricing() {
    holds_should_be_added_and_cleared(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
    })
    .await;
}

#[tokio::test]
async fn holds_should_be_added_and_cleared_classic_pricing() {
    holds_should_be_added_and_cleared(PricingMode::Classic {
        payment_amount: 5000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn holds_should_be_added_and_cleared(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = U512::from(
        test.chainspec()
            .transaction_config
            .native_transfer_minimum_motes,
    );

    // transfer from bob to charlie
    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        txn_pricing_mode,
        transfer_amount,
    );

    let expected_transfer_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .mint_costs()
            .transfer
            .into(),
    );
    let expected_transfer_cost = expected_transfer_gas * min_gas_price as u64;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (_, bob_initial_balance, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result); // transaction should have succeeded.
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
    );

    assert_eq!(
        initial_total_supply,
        test.get_total_supply(Some(block_height)),
        "total supply should remain unchanged"
    );

    // Get the current balances after the transaction and check them.
    let (_, bob_current_balance, charlie_balance) = test.get_balances(Some(block_height));
    assert_eq!(
        charlie_balance
            .expect("Expected Charlie to have a balance")
            .total,
        transfer_amount,
        "charlie's balance should equal transfer amount"
    );
    assert_ne!(
        bob_current_balance.available, bob_current_balance.total,
        "total and available should NOT be equal at this point"
    );
    assert_eq!(
        bob_initial_balance.total,
        bob_current_balance.total + transfer_amount,
        "total balance should be original total balance - transferred amount"
    );
    assert_eq!(
        bob_initial_balance.total,
        bob_current_balance.available + expected_transfer_cost + transfer_amount,
        "diff from initial balance should equal available + cost + transfer_amount"
    );

    test.fixture
        .run_until_block_height(block_height + 5, ONE_MIN)
        .await;
    let (_, bob_balance, _) = test.get_balances(Some(block_height + 5));
    assert_eq!(
        bob_balance.available, bob_balance.total,
        "total and available should be equal at this point"
    );
}

#[tokio::test]
async fn fee_holds_are_amortized() {
    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Amortized)
        .with_balance_hold_interval(TimeDiff::from_seconds(10));

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;
    let txn = invalid_wasm_txn(
        BOB_SECRET_KEY.clone(),
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    // Fixed transaction pricing.
    let expected_transaction_gas: u64 = test
        .chainspec()
        .system_costs_config
        .standard_transaction_limit();

    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0), /* expect that this transaction doesn't consume any gas since it has
                      * invalid wasm. */
    );

    // This transaction consumed 0 gas, the unspent gas is equal to the limit, so we apply the
    // refund ratio to the full transaction cost.
    let refund_amount: U512 = (refund_ratio * Ratio::from(expected_transaction_cost))
        .to_integer()
        .into();

    // We set it up so that the refunds are burnt so check this.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - refund_amount
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob doesn't get a refund. The refund is burnt. A hold is put in in place for the
    // transaction cost.
    let bob_balance_hold = U512::from(expected_transaction_cost) - refund_amount;
    let bob_expected_total_balance = bob_initial_balance.total - refund_amount;
    let bob_expected_available_balance = bob_current_balance.total - bob_balance_hold;

    // Alice should't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );

    let bob_prev_available_balance = bob_current_balance.available;
    test.fixture
        .run_until_block_height(block_height + 1, ONE_MIN)
        .await;
    let (_, bob_balance, _) = test.get_balances(Some(block_height + 1));
    assert!(
        bob_prev_available_balance < bob_balance.available,
        "available should have increased since some part of the hold should have been amortized"
    );

    // Check to see if more holds have amortized.
    let bob_prev_available_balance = bob_current_balance.available;
    test.fixture
        .run_until_block_height(block_height + 3, ONE_MIN)
        .await;
    let (_, bob_balance, _) = test.get_balances(Some(block_height + 3));
    assert!(
        bob_prev_available_balance < bob_balance.available,
        "available should have increased since some part of the hold should have been amortized"
    );

    // After 10s (10 blocks in this case) the holds should have been completely amortized
    test.fixture
        .run_until_block_height(block_height + 10, ONE_MIN)
        .await;
    let (_, bob_balance, _) = test.get_balances(Some(block_height + 10));
    assert_eq!(
        bob_balance.total, bob_balance.available,
        "available should have increased since some part of the hold should have been amortized"
    );
}

#[tokio::test]
async fn sufficient_balance_is_available_after_amortization() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Amortized)
        .with_balance_hold_interval(TimeDiff::from_seconds(10));

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_cost: U512 =
        U512::from(test.chainspec().system_costs_config.mint_costs().transfer) * MIN_GAS_PRICE;
    let min_transfer_amount = U512::from(
        test.chainspec()
            .transaction_config
            .native_transfer_minimum_motes,
    );
    let half_transfer_cost =
        (Ratio::new(U512::from(1), U512::from(2)) * transfer_cost).to_integer();

    // Fund Charlie with some token.
    let transfer_amount = min_transfer_amount * 2 + transfer_cost + half_transfer_cost;
    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
        transfer_amount,
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));

    let charlie_balance = test.get_balances(Some(block_height)).2.unwrap();
    assert_eq!(
        charlie_balance.available.clone(),
        charlie_balance.total.clone()
    );
    assert_eq!(charlie_balance.available.clone(), transfer_amount);

    // Now Charlie has balance to do 2 transfers of the minimum amount but can't pay for both as the
    // same time. Let's say the min transfer amount is 2_500_000_000 and the cost of a transfer
    // is 50_000. Charlie now has 5_000_075_000 as set up above. He can transfer 2_500_000_000
    // which will put a hold of 50_000. His available balance would be 2_500_025_000.
    // He can't issue a new transfer of 2_500_000_000 right away because he doesn't have enough
    // balance to pay for the transfer. He'll need to wait until at least half of the holds
    // amortize. In this case he needs to wait half of the amortization time for 25_000 to
    // become available to him. After this period, he will have 2_500_050_000 available which
    // will allow him to do another transfer.
    let txn = transfer_txn(
        CHARLIE_SECRET_KEY.clone(),
        &BOB_PUBLIC_KEY,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
        min_transfer_amount,
    );
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));

    let charlie_balance = test.get_balances(Some(block_height)).2.unwrap();
    assert_eq!(
        charlie_balance.total.clone(),
        min_transfer_amount + transfer_cost + half_transfer_cost, /* one `min_transfer_amount`
                                                                   * should have gone to Bob. */
    );
    assert_eq!(
        charlie_balance.available.clone(),
        min_transfer_amount + half_transfer_cost, // transfer cost should be held.
    );

    // Let's wait for about 5 sec (5 blocks in this case) which should provide enough time for at
    // half of the holds to get amortized.
    test.fixture
        .run_until_block_height(block_height + 5, ONE_MIN)
        .await;
    let charlie_balance = test.get_balances(Some(block_height + 5)).2.unwrap();
    assert!(
        charlie_balance.available >= min_transfer_amount + transfer_cost, /* right now he should
                                                                           * have enough to make
                                                                           * a transfer. */
    );
    assert!(
        charlie_balance.available < charlie_balance.total, /* some of the holds
                                                            * should still be in
                                                            * place. */
    );

    // Send another transfer to Bob for `min_transfer_amount`.
    let txn = transfer_txn(
        CHARLIE_SECRET_KEY.clone(),
        &BOB_PUBLIC_KEY,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
        min_transfer_amount,
    );
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result)); // We expect this transfer to succeed since Charlie has enough balance.
    let charlie_balance = test.get_balances(Some(block_height)).2.unwrap();
    assert_eq!(
        charlie_balance.total.clone(),
        transfer_cost + half_transfer_cost, // two `min_transfer_amount` should have gone to Bob.
    );
}

#[tokio::test]
async fn validator_credit_is_written_and_cleared_after_auction() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_cost: U512 =
        U512::from(test.chainspec().system_costs_config.mint_costs().transfer) * MIN_GAS_PRICE;
    let min_transfer_amount = U512::from(
        test.chainspec()
            .transaction_config
            .native_transfer_minimum_motes,
    );
    let half_transfer_cost =
        (Ratio::new(U512::from(1), U512::from(2)) * transfer_cost).to_integer();

    // Fund Charlie with some token.
    let transfer_amount = min_transfer_amount * 2 + transfer_cost + half_transfer_cost;
    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
        },
        transfer_amount,
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));

    let charlie_balance = test.get_balances(Some(block_height)).2.unwrap();
    assert_eq!(
        charlie_balance.available.clone(),
        charlie_balance.total.clone()
    );
    assert_eq!(charlie_balance.available.clone(), transfer_amount);

    let bids =
        get_bids(&mut test.fixture, Some(block_height)).expect("Expected to get some bid records.");

    let _ = bids
        .into_iter()
        .find(|bid_kind| match bid_kind {
            BidKind::Credit(credit) => {
                credit.amount() == transfer_cost
                    && credit.validator_public_key() == &*ALICE_PUBLIC_KEY // Alice is the proposer.
            }
            _ => false,
        })
        .expect("Expected to find the credit for the consumed transfer cost in the bid records.");

    test.fixture
        .run_until_consensus_in_era(
            ERA_ONE.saturating_add(test.chainspec().core_config.auction_delay),
            ONE_MIN,
        )
        .await;

    // Check that the credits were cleared after the auction.
    let bids = get_bids(&mut test.fixture, None).expect("Expected to get some bid records.");
    assert!(!bids
        .into_iter()
        .any(|bid| matches!(bid, BidKind::Credit(_))));
}

#[tokio::test]
async fn add_and_withdraw_bid_transaction() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_cost: U512 =
        U512::from(test.chainspec().system_costs_config.mint_costs().transfer) * MIN_GAS_PRICE;
    let min_transfer_amount = U512::from(
        test.chainspec()
            .transaction_config
            .native_transfer_minimum_motes,
    );
    let half_transfer_cost =
        (Ratio::new(U512::from(1), U512::from(2)) * transfer_cost).to_integer();
    let transfer_amount = min_transfer_amount * 2 + transfer_cost + half_transfer_cost;
    let minimum_delegation_amount = test.chainspec().core_config.minimum_delegation_amount;
    let maximum_delegation_amount = test.chainspec().core_config.maximum_delegation_amount;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_add_bid(
            PublicKey::from(&**BOB_SECRET_KEY),
            0,
            transfer_amount,
            minimum_delegation_amount,
            maximum_delegation_amount,
        )
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (_, _bob_initial_balance, _) = test.get_balances(None);
    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));

    test.fixture
        .run_until_consensus_in_era(ERA_TWO, ONE_MIN)
        .await;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_withdraw_bid(PublicKey::from(&**BOB_SECRET_KEY), transfer_amount)
            .unwrap()
            .with_chain_name(CHAIN_NAME)
            .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
            .build()
            .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    println!("{:?}", exec_result);
    assert!(exec_result_is_success(&exec_result));
}

#[tokio::test]
async fn delegate_and_undelegate_bid_transaction() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let delegate_amount = U512::from(500_000_000_000u64);

    let mut txn = Transaction::from(
        TransactionV1Builder::new_delegate(
            PublicKey::from(&**BOB_SECRET_KEY),
            PublicKey::from(&**ALICE_SECRET_KEY),
            delegate_amount,
        )
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_undelegate(
            PublicKey::from(&**BOB_SECRET_KEY),
            PublicKey::from(&**ALICE_SECRET_KEY),
            delegate_amount,
        )
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));
}
