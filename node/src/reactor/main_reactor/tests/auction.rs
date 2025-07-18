use std::sync::Arc;

use crate::reactor::main_reactor::tests::{
    configs_override::ConfigsOverride, fixture::TestFixture, initial_stakes::InitialStakes,
    ERA_ONE, ERA_TWO, ONE_MIN, TEN_SECS,
};
use casper_types::{
    execution::{ExecutionResult, TransformKindV2},
    system::{auction::BidAddr, AUCTION},
    Deploy, Key, PublicKey, StoredValue, TimeDiff, Timestamp, Transaction, U512,
};

#[tokio::test]
async fn run_withdraw_bid_network() {
    let alice_stake = 200_000_000_000_u64;
    let initial_stakes = InitialStakes::FromVec(vec![alice_stake.into(), 10_000_000_000]);

    let unbonding_delay = 2;

    let mut fixture = TestFixture::new(
        initial_stakes,
        Some(ConfigsOverride {
            unbonding_delay,
            ..Default::default()
        }),
    )
    .await;
    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);

    // Wait for all nodes to complete block 0.
    fixture.run_until_block_height(0, ONE_MIN).await;

    // Ensure our post genesis assumption that Alice has a bid is correct.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);

    // Create & sign deploy to withdraw Alice's full stake.
    let mut deploy = Deploy::withdraw_bid(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        alice_public_key.clone(),
        alice_stake.into(),
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&alice_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    // Ensure execution succeeded and that there is a Prune transform for the bid's key.
    let bid_key = Key::BidAddr(BidAddr::from(alice_public_key.clone()));
    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Prune(prune_key) => prune_key == &bid_key,
            _ => false,
        })
        .expect("should have a prune record for bid");

    // Crank the network forward until the era ends.
    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;

    // The bid record should have been pruned once unbonding ran.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, false);

    // Crank the network forward until the unbonding queue is processed.
    fixture
        .run_until_stored_switch_block_header(
            ERA_ONE.saturating_add(unbonding_delay + 1),
            ONE_MIN * 2,
        )
        .await;
}

#[tokio::test]
async fn should_error_on_validator_unbond_to_large() {
    let alice_stake = 200_000_000_000_u64;
    let initial_stakes = InitialStakes::FromVec(vec![alice_stake.into(), 10_000_000_000]);

    let unbonding_delay = 2;

    let mut fixture = TestFixture::new(
        initial_stakes,
        Some(ConfigsOverride {
            unbonding_delay,
            ..Default::default()
        }),
    )
    .await;
    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);

    // Wait for all nodes to complete block 0.
    fixture.run_until_block_height(0, ONE_MIN).await;

    // Ensure our post genesis assumption that Alice has a bid is correct.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);

    let too_much_stake = alice_stake + 1;

    // Create & sign deploy to withdraw MORE than Alice's full stake.
    let mut deploy = Deploy::withdraw_bid(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        alice_public_key.clone(),
        too_much_stake.into(),
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&alice_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    let result = fixture.transaction_execution_result(&txn_hash);

    if let ExecutionResult::V2(exec_result) = result {
        let msg = exec_result
            .error_message
            .expect("error message should not be none");

        assert_eq!(
            msg, "ApiError::AuctionError(UnbondTooLarge) [64532]",
            "{}",
            msg
        );
    } else {
        panic!("unexpected execution result");
    }
}

#[tokio::test]
async fn run_undelegate_bid_network() {
    let alice_stake = 200_000_000_000_u64;
    let bob_stake = 300_000_000_000_u64;
    let initial_stakes = InitialStakes::FromVec(vec![alice_stake.into(), bob_stake.into()]);

    let unbonding_delay = 2;

    let mut fixture = TestFixture::new(
        initial_stakes,
        Some(ConfigsOverride {
            unbonding_delay,
            ..Default::default()
        }),
    )
    .await;
    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_public_key = PublicKey::from(&*fixture.node_contexts[1].secret_key);

    // Wait for all nodes to complete block 0.
    fixture.run_until_block_height(0, ONE_MIN).await;

    // Ensure our post genesis assumption that Alice and Bob have bids is correct.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, None, true);
    // Alice should not have a delegation bid record for Bob (yet).
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), false);

    // Have Alice delegate to Bob.
    //
    // Note, in the real world validators usually don't also delegate to other validators,  but in
    // this test fixture the only accounts in the system are those created for genesis validators.
    let alice_delegation_amount =
        U512::from(fixture.chainspec.core_config.minimum_delegation_amount);
    let mut deploy = Deploy::delegate(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        bob_public_key.clone(),
        alice_public_key.clone(),
        alice_delegation_amount,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&alice_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    // Ensure execution succeeded and that there is a Write transform for the bid's key.
    let bid_key = Key::BidAddr(BidAddr::new_from_public_keys(
        &bob_public_key,
        Some(&alice_public_key),
    ));
    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Write(StoredValue::BidKind(bid_kind)) => {
                Key::from(bid_kind.bid_addr()) == bid_key
            }
            _ => false,
        })
        .expect("should have a write record for delegate bid");

    // Alice should now have a delegation bid record for Bob.
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), true);

    // Create & sign transaction to undelegate from Alice to Bob.
    let mut deploy = Deploy::undelegate(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        bob_public_key.clone(),
        alice_public_key.clone(),
        alice_delegation_amount,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&alice_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    // Ensure execution succeeded and that there is a Prune transform for the bid's key.
    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Prune(prune_key) => prune_key == &bid_key,
            _ => false,
        })
        .expect("should have a prune record for undelegated bid");

    // Crank the network forward until the era ends.
    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;

    // Ensure the validator records are still present but the undelegated bid is gone.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), false);

    // Crank the network forward until the unbonding queue is processed.
    fixture
        .run_until_stored_switch_block_header(
            ERA_ONE.saturating_add(unbonding_delay + 1),
            ONE_MIN * 2,
        )
        .await;
}

#[tokio::test]
async fn run_redelegate_bid_network() {
    let alice_stake = 200_000_000_000_u64;
    let bob_stake = 300_000_000_000_u64;
    let charlie_stake = 300_000_000_000_u64;
    let initial_stakes = InitialStakes::FromVec(vec![
        alice_stake.into(),
        bob_stake.into(),
        charlie_stake.into(),
    ]);

    let spec_override = ConfigsOverride {
        unbonding_delay: 1,
        minimum_era_height: 5,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;
    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_public_key = PublicKey::from(&*fixture.node_contexts[1].secret_key);
    let charlie_public_key = PublicKey::from(&*fixture.node_contexts[2].secret_key);

    // Wait for all nodes to complete block 0.
    fixture.run_until_block_height(0, ONE_MIN).await;

    // Ensure our post genesis assumption that Alice, Bob and Charlie have bids is correct.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, None, true);
    fixture.check_bid_existence_at_tip(&charlie_public_key, None, true);
    // Alice should not have a delegation bid record for Bob or Charlie (yet).
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), false);
    fixture.check_bid_existence_at_tip(&charlie_public_key, Some(&alice_public_key), false);

    // Have Alice delegate to Bob.
    let alice_delegation_amount =
        U512::from(fixture.chainspec.core_config.minimum_delegation_amount);
    let mut deploy = Deploy::delegate(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        bob_public_key.clone(),
        alice_public_key.clone(),
        alice_delegation_amount,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&alice_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, ONE_MIN)
        .await;

    // Ensure execution succeeded and that there is a Write transform for the bid's key.
    let bid_key = Key::BidAddr(BidAddr::new_from_public_keys(
        &bob_public_key,
        Some(&alice_public_key),
    ));

    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Write(StoredValue::BidKind(bid_kind)) => {
                Key::from(bid_kind.bid_addr()) == bid_key
            }
            _ => false,
        })
        .expect("should have a write record for delegate bid");

    // Alice should now have a delegation bid record for Bob.
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), true);

    // Create & sign transaction to undelegate Alice from Bob and delegate to Charlie.
    let mut deploy = Deploy::redelegate(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        bob_public_key.clone(),
        alice_public_key.clone(),
        charlie_public_key.clone(),
        alice_delegation_amount,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );

    deploy.sign(&alice_secret_key);
    let transaction = Transaction::Deploy(deploy);
    let transaction_hash = transaction.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(transaction).await;
    fixture
        .run_until_executed_transaction(&transaction_hash, TEN_SECS)
        .await;

    // Ensure execution succeeded and that there is a Prune transform for the bid's key.
    fixture
        .successful_execution_transforms(&transaction_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Prune(prune_key) => prune_key == &bid_key,
            _ => false,
        })
        .expect("should have a prune record for undelegated bid");

    // Original delegation bid should be removed.
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), false);
    // Redelegate doesn't occur until after unbonding delay elapses.
    fixture.check_bid_existence_at_tip(&charlie_public_key, Some(&alice_public_key), false);

    // Crank the network forward to run out the unbonding delay.
    // First, close out the era the redelegate was processed in.
    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;
    // The undelegate is in the unbonding queue.
    fixture.check_bid_existence_at_tip(&charlie_public_key, Some(&alice_public_key), false);
    // Unbonding delay is 1 on this test network, so step 1 more era.
    fixture
        .run_until_stored_switch_block_header(ERA_TWO, ONE_MIN)
        .await;

    // Ensure the validator records are still present.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, None, true);
    // Ensure redelegated bid exists.
    fixture.check_bid_existence_at_tip(&charlie_public_key, Some(&alice_public_key), true);
}
