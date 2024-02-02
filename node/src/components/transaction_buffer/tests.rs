use prometheus::Registry;
use rand::Rng;

use casper_types::{testing::TestRng, Deploy, EraId, TestBlockBuilder, TimeDiff, Transaction};

use super::*;
use crate::{
    effect::announcements::TransactionBufferAnnouncement::{self, TransactionsExpired},
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    types::FinalizedBlock,
    utils,
};

enum TransactionType {
    Transfer,
    Standard,
    Random,
}

// Generates valid transactions
fn create_valid_transactions(
    rng: &mut TestRng,
    size: usize,
    transaction_type: TransactionType,
    strict_timestamp: Option<Timestamp>,
    with_ttl: Option<TimeDiff>,
) -> Vec<Transaction> {
    let mut transactions = Vec::with_capacity(size);

    for _ in 0..size {
        let transaction_ttl = match with_ttl {
            Some(ttl) => ttl,
            None => TimeDiff::from_seconds(rng.gen_range(30..100)),
        };
        let transaction_timestamp = match strict_timestamp {
            Some(timestamp) => timestamp,
            None => Timestamp::now(),
        };
        match transaction_type {
            TransactionType::Transfer => {
                transactions.push(Transaction::from(
                    Deploy::random_valid_native_transfer_with_timestamp_and_ttl(
                        rng,
                        transaction_timestamp,
                        transaction_ttl,
                    ),
                ));
            }
            TransactionType::Standard => {
                if strict_timestamp.is_some() {
                    unimplemented!();
                }
                let transaction =
                    Transaction::from(Deploy::random_with_valid_session_package_by_name(rng));
                assert!(transaction.verify().is_ok());
                transactions.push(transaction);
            }
            TransactionType::Random => {
                transactions.push(Transaction::from(Deploy::random_with_timestamp_and_ttl(
                    rng,
                    transaction_timestamp,
                    transaction_ttl,
                )));
            }
        }
    }

    transactions
}

fn create_invalid_transactions(rng: &mut TestRng, size: usize) -> Vec<Transaction> {
    let mut transactions =
        create_valid_transactions(rng, size, TransactionType::Random, None, None);

    for transaction in transactions.iter_mut() {
        transaction.invalidate();
    }

    transactions
}

/// Checks sizes of the transaction_buffer containers. Also checks the metrics recorded.
#[track_caller]
fn assert_container_sizes(
    transaction_buffer: &TransactionBuffer,
    expected_buffer: usize,
    expected_dead: usize,
    expected_held: usize,
) {
    assert_eq!(
        transaction_buffer.buffer.len(),
        expected_buffer,
        "wrong `buffer` length"
    );
    assert_eq!(
        transaction_buffer.dead.len(),
        expected_dead,
        "wrong `dead` length"
    );
    assert_eq!(
        transaction_buffer
            .hold
            .values()
            .map(|transactions| transactions.len())
            .sum::<usize>(),
        expected_held,
        "wrong `hold` length"
    );
    assert_eq!(
        transaction_buffer.metrics.total_transactions.get(),
        expected_buffer as i64,
        "wrong `metrics.total_transactions`"
    );
    assert_eq!(
        transaction_buffer.metrics.held_transactions.get(),
        expected_held as i64,
        "wrong `metrics.held_transactions`"
    );
    assert_eq!(
        transaction_buffer.metrics.dead_transactions.get(),
        expected_dead as i64,
        "wrong `metrics.dead_transactions`"
    );
}

#[test]
fn register_transaction_and_check_size() {
    let mut rng = TestRng::new();
    let mut transaction_buffer = TransactionBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    // Try to register valid transactions
    let num_valid_transactions: usize = rng.gen_range(50..500);
    let valid_transactions = create_valid_transactions(
        &mut rng,
        num_valid_transactions,
        TransactionType::Random,
        None,
        None,
    );
    valid_transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(&transaction_buffer, valid_transactions.len(), 0, 0);

    // Try to register invalid transactions
    let num_invalid_transactions: usize = rng.gen_range(10..100);
    let invalid_transactions = create_invalid_transactions(&mut rng, num_invalid_transactions);
    invalid_transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(&transaction_buffer, valid_transactions.len(), 0, 0);

    // Try to register a duplicate transaction
    let duplicate_transaction = valid_transactions
        .get(rng.gen_range(0..num_valid_transactions))
        .unwrap()
        .clone();
    transaction_buffer.register_transaction(duplicate_transaction);
    assert_container_sizes(&transaction_buffer, valid_transactions.len(), 0, 0);

    // Insert transaction without footprint
    let bad_transaction = Transaction::from(Deploy::random_without_payment_amount(&mut rng));
    transaction_buffer.register_transaction(bad_transaction);
    assert_container_sizes(&transaction_buffer, valid_transactions.len(), 0, 0);
}

#[test]
fn register_block_with_valid_transactions() {
    let mut rng = TestRng::new();
    let mut transaction_buffer = TransactionBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    let transactions = create_valid_transactions(&mut rng, 10, TransactionType::Random, None, None);
    let txns: Vec<_> = transactions.to_vec();
    let era_id = EraId::new(rng.gen_range(0..6));
    let height = era_id.value() * 10 + rng.gen_range(0..10);
    let is_switch = rng.gen_bool(0.1);
    let block = TestBlockBuilder::new()
        .era(era_id)
        .height(height)
        .switch_block(is_switch)
        .transactions(&txns)
        .build(&mut rng);

    transaction_buffer.register_block(&block);
    assert_container_sizes(
        &transaction_buffer,
        transactions.len(),
        transactions.len(),
        0,
    );
}

#[test]
fn register_finalized_block_with_valid_transactions() {
    let mut rng = TestRng::new();
    let mut transaction_buffer = TransactionBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    let transactions = create_valid_transactions(&mut rng, 10, TransactionType::Random, None, None);
    let txns: Vec<_> = transactions.to_vec();
    let block = FinalizedBlock::random(&mut rng, &txns);

    transaction_buffer.register_block_finalized(&block);
    assert_container_sizes(
        &transaction_buffer,
        transactions.len(),
        transactions.len(),
        0,
    );
}

#[test]
fn get_proposable_transactions() {
    let mut rng = TestRng::new();
    let mut transaction_buffer = TransactionBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    // populate transaction buffer with some transactions
    let transactions = create_valid_transactions(&mut rng, 50, TransactionType::Random, None, None);
    transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(&transaction_buffer, transactions.len(), 0, 0);

    // Create a block with some transactions and register it with the transaction_buffer
    let block_transactions =
        create_valid_transactions(&mut rng, 10, TransactionType::Random, None, None);
    let txns: Vec<_> = block_transactions.to_vec();
    let block = FinalizedBlock::random(&mut rng, &txns);
    transaction_buffer.register_block_finalized(&block);
    assert_container_sizes(
        &transaction_buffer,
        transactions.len() + block_transactions.len(),
        block_transactions.len(),
        0,
    );

    // Check which transactions are proposable. Should return the transactions that were not
    // included in the block since those should be dead.
    let proposable = transaction_buffer.proposable();
    assert_eq!(proposable.len(), transactions.len());
    let proposable_transaction_hashes: HashSet<_> = proposable
        .iter()
        .map(|(th, _)| th.transaction_hash())
        .collect();
    for transaction in transactions.iter() {
        assert!(proposable_transaction_hashes.contains(&transaction.hash()));
    }

    // Get an appendable block. This should put the transactions on hold.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert_eq!(transaction_buffer.hold.len(), 1);
    assert_container_sizes(
        &transaction_buffer,
        transactions.len() + block_transactions.len(),
        block_transactions.len(),
        appendable_block.transaction_and_transfer_set().len(),
    );

    // Check that held blocks are not proposable
    let proposable = transaction_buffer.proposable();
    assert_eq!(
        proposable.len(),
        transactions.len() - appendable_block.transaction_and_transfer_set().len()
    );
    for transaction in proposable.iter() {
        assert!(!appendable_block
            .transaction_and_transfer_set()
            .contains(&transaction.0.transaction_hash()));
    }
}

#[test]
fn get_appendable_block_with_native_transfers() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_transfer_count: 200,
        block_max_staking_count: 0,
        block_max_install_upgrade_count: 0,
        block_max_standard_count: 10,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut transaction_buffer =
        TransactionBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();
    get_appendable_block(
        &mut rng,
        &mut transaction_buffer,
        TransactionType::Transfer,
        transaction_config.block_max_transfer_count as usize,
    );
}

#[test]
fn get_appendable_block_with_standard_transactions() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_transfer_count: 200,
        block_max_staking_count: 0,
        block_max_install_upgrade_count: 0,
        block_max_standard_count: 10,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut transaction_buffer =
        TransactionBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();
    get_appendable_block(
        &mut rng,
        &mut transaction_buffer,
        TransactionType::Standard,
        transaction_config.block_max_standard_count as usize,
    );
}

#[test]
fn get_appendable_block_with_random_transactions() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_transfer_count: 200,
        block_max_staking_count: 0,
        block_max_install_upgrade_count: 0,
        block_max_standard_count: 10,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut transaction_buffer =
        TransactionBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();
    get_appendable_block(
        &mut rng,
        &mut transaction_buffer,
        TransactionType::Random,
        (transaction_config.block_max_transfer_count + transaction_config.block_max_standard_count)
            as usize,
    );
}

fn get_appendable_block(
    rng: &mut TestRng,
    transaction_buffer: &mut TransactionBuffer,
    transaction_type: TransactionType,
    transaction_limit: usize,
) {
    // populate transaction buffer with more transfers than a block can fit
    let transactions =
        create_valid_transactions(rng, transaction_limit + 50, transaction_type, None, None);
    transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(transaction_buffer, transactions.len(), 0, 0);

    // now check how many transfers were added in the block; should not exceed the config limits.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert!(appendable_block.transaction_and_transfer_set().len() <= transaction_limit,);
    assert_eq!(transaction_buffer.hold.len(), 1);
    assert_container_sizes(
        transaction_buffer,
        transactions.len(),
        0,
        appendable_block.transaction_and_transfer_set().len(),
    );
}

#[test]
fn register_transactions_and_blocks() {
    let mut rng = TestRng::new();
    let mut transaction_buffer = TransactionBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    // try to register valid transactions
    let num_valid_transactions: usize = rng.gen_range(50..500);
    let valid_transactions = create_valid_transactions(
        &mut rng,
        num_valid_transactions,
        TransactionType::Random,
        None,
        None,
    );
    valid_transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(&transaction_buffer, valid_transactions.len(), 0, 0);

    // register a block with transactions
    let block_transaction =
        create_valid_transactions(&mut rng, 5, TransactionType::Random, None, None);
    let txns: Vec<_> = block_transaction.to_vec();
    let era = rng.gen_range(0..6);
    let height = era * 10 + rng.gen_range(0..10);
    let is_switch = rng.gen_bool(0.1);

    let block = TestBlockBuilder::new()
        .era(era)
        .height(height)
        .switch_block(is_switch)
        .transactions(&txns)
        .build(&mut rng);

    transaction_buffer.register_block(&block);
    assert_container_sizes(
        &transaction_buffer,
        block_transaction.len() + valid_transactions.len(),
        block_transaction.len(),
        0,
    );

    // try to register the transactions of the block again. Should not work since those transactions
    // are dead.
    block_transaction
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(
        &transaction_buffer,
        block_transaction.len() + valid_transactions.len(),
        block_transaction.len(),
        0,
    );

    let pre_proposal_timestamp = Timestamp::now();

    // get an appendable block. This should put the transactions on hold.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert_eq!(transaction_buffer.hold.len(), 1);
    assert_container_sizes(
        &transaction_buffer,
        block_transaction.len() + valid_transactions.len(),
        block_transaction.len(),
        appendable_block.transaction_and_transfer_set().len(),
    );

    // try to register held transactions again.
    let mut held_transactions = valid_transactions
        .iter()
        .cloned()
        .filter(|transaction| {
            appendable_block
                .transaction_and_transfer_set()
                .contains(&transaction.hash())
        })
        .peekable();
    assert!(held_transactions.peek().is_some());
    held_transactions.for_each(|transaction| transaction_buffer.register_transaction(transaction));
    assert_container_sizes(
        &transaction_buffer,
        block_transaction.len() + valid_transactions.len(),
        block_transaction.len(),
        appendable_block.transaction_and_transfer_set().len(),
    );

    // test if transactions held for proposed blocks which did not get finalized in time
    // are eligible again
    let count = rng.gen_range(0..11);
    let txns: Vec<_> = std::iter::repeat_with(|| Transaction::Deploy(Deploy::random(&mut rng)))
        .take(count)
        .collect();
    let block = FinalizedBlock::random_with_specifics(
        &mut rng,
        EraId::from(2),
        25,
        false,
        pre_proposal_timestamp,
        &txns,
    );
    transaction_buffer.register_block_finalized(&block);
    assert_container_sizes(
        &transaction_buffer,
        block_transaction.len() + valid_transactions.len() + block.all_transactions().count(),
        block_transaction.len() + block.all_transactions().count(),
        0,
    );
}

/// Event for the mock reactor.
#[derive(Debug)]
enum ReactorEvent {
    TransactionBufferAnnouncement(TransactionBufferAnnouncement),
    Event(Event),
}

impl From<TransactionBufferAnnouncement> for ReactorEvent {
    fn from(req: TransactionBufferAnnouncement) -> ReactorEvent {
        ReactorEvent::TransactionBufferAnnouncement(req)
    }
}

impl From<Event> for ReactorEvent {
    fn from(req: Event) -> ReactorEvent {
        ReactorEvent::Event(req)
    }
}

struct MockReactor {
    scheduler: &'static Scheduler<ReactorEvent>,
}

impl MockReactor {
    fn new() -> Self {
        MockReactor {
            scheduler: utils::leak(Scheduler::new(QueueKind::weights(), None)),
        }
    }

    async fn expect_transaction_buffer_expire_announcement(
        &self,
        should_be_expired: &HashSet<TransactionHash>,
    ) {
        let ((_ancestor, reactor_event), _) = self.scheduler.pop().await;
        match reactor_event {
            ReactorEvent::TransactionBufferAnnouncement(TransactionsExpired(expired)) => {
                let expired_set = HashSet::from_iter(expired);
                assert_eq!(&expired_set, should_be_expired);
            }
            _ => {
                unreachable!();
            }
        };
    }
}

#[tokio::test]
async fn expire_transactions_and_check_announcement() {
    let mut rng = TestRng::new();
    let mut transaction_buffer = TransactionBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    let reactor = MockReactor::new();
    let event_queue_handle = EventQueueHandle::without_shutdown(reactor.scheduler);
    let effect_builder = EffectBuilder::new(event_queue_handle);

    // generate and register some already expired transactions
    let ttl = TimeDiff::from_seconds(rng.gen_range(30..300));
    let past_timestamp = Timestamp::now()
        .saturating_sub(ttl)
        .saturating_sub(TimeDiff::from_seconds(5));

    let num_transactions: usize = rng.gen_range(5..50);
    let expired_transactions = create_valid_transactions(
        &mut rng,
        num_transactions,
        TransactionType::Transfer,
        Some(past_timestamp),
        Some(ttl),
    );
    expired_transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(&transaction_buffer, expired_transactions.len(), 0, 0);

    // include the last expired transaction in a block and register it
    let era = rng.gen_range(0..6);
    let expired_txns: Vec<_> = expired_transactions.to_vec();
    let block = TestBlockBuilder::new()
        .era(era)
        .height(era * 10 + rng.gen_range(0..10))
        .transactions(expired_txns.last())
        .build(&mut rng);

    transaction_buffer.register_block(&block);
    assert_container_sizes(&transaction_buffer, expired_transactions.len(), 1, 0);

    // generate and register some valid transactions
    let transactions = create_valid_transactions(
        &mut rng,
        num_transactions,
        TransactionType::Transfer,
        None,
        None,
    );
    transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(
        &transaction_buffer,
        transactions.len() + expired_transactions.len(),
        1,
        0,
    );

    // expire transactions and check that they were announced as expired
    let mut effects = transaction_buffer.expire(effect_builder);
    tokio::spawn(effects.remove(0)).await.unwrap();

    // the transactions which should be announced as expired are all the expired ones not in a
    // block, i.e. all but the last one of `expired_transactions`
    let expired_transaction_hashes: HashSet<_> = expired_transactions
        .iter()
        .take(expired_transactions.len() - 1)
        .map(|transaction| transaction.hash())
        .collect();
    reactor
        .expect_transaction_buffer_expire_announcement(&expired_transaction_hashes)
        .await;

    // the valid transactions should still be in the buffer
    assert_container_sizes(&transaction_buffer, transactions.len(), 0, 0);
}
