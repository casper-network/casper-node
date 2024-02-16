use prometheus::Registry;
use rand::{seq::SliceRandom, Rng};

use casper_types::{
    testing::TestRng, Deploy, EraId, TestBlockBuilder, TimeDiff, Transaction, TransactionV1,
};

use super::*;
use crate::{
    effect::announcements::TransactionBufferAnnouncement::{self, TransactionsExpired},
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    types::FinalizedBlock,
    utils,
};

#[derive(Debug)]
enum TransactionCategory {
    TransferLegacy,
    Transfer,
    StandardLegacy,
    Standard,
    InstallUpgrade,
    Staking,
}

impl TransactionCategory {
    fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..6) {
            0 => TransactionCategory::TransferLegacy,
            1 => TransactionCategory::Transfer,
            2 => TransactionCategory::StandardLegacy,
            3 => TransactionCategory::Standard,
            4 => TransactionCategory::InstallUpgrade,
            _ => TransactionCategory::Staking,
        }
    }
}

fn get_appendable_block(
    rng: &mut TestRng,
    transaction_buffer: &mut TransactionBuffer,
    categories: impl Iterator<Item = &'static TransactionCategory>,
    transaction_limit: usize,
) {
    let transactions: Vec<_> = categories
        .take(transaction_limit + 50)
        .into_iter()
        .map(|category| create_valid_transaction(rng, category, None, None))
        .collect();
    transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(transaction_buffer, transactions.len(), 0, 0);

    // now check how many transfers were added in the block; should not exceed the config limits.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now(), EraId::new(1));
    assert!(appendable_block.transaction_and_transfer_set().len() <= transaction_limit,);
    assert_eq!(transaction_buffer.hold.len(), 1);
    assert_container_sizes(
        transaction_buffer,
        transactions.len(),
        0,
        appendable_block.transaction_and_transfer_set().len(),
    );
}

// Generates valid transactions
fn create_valid_transaction(
    rng: &mut TestRng,
    transaction_category: &TransactionCategory,
    strict_timestamp: Option<Timestamp>,
    with_ttl: Option<TimeDiff>,
) -> Transaction {
    let transaction_ttl = match with_ttl {
        Some(ttl) => ttl,
        None => TimeDiff::from_seconds(rng.gen_range(30..100)),
    };
    let transaction_timestamp = match strict_timestamp {
        Some(timestamp) => timestamp,
        None => Timestamp::now(),
    };

    match transaction_category {
        TransactionCategory::TransferLegacy => {
            Transaction::Deploy(Deploy::random_valid_native_transfer_with_timestamp_and_ttl(
                rng,
                transaction_timestamp,
                transaction_ttl,
            ))
        }
        TransactionCategory::Transfer => Transaction::V1(TransactionV1::random_transfer(
            rng,
            strict_timestamp,
            with_ttl,
        )),
        TransactionCategory::StandardLegacy => {
            Transaction::Deploy(match (strict_timestamp, with_ttl) {
                (Some(timestamp), Some(ttl)) if Timestamp::now() > timestamp + ttl => {
                    Deploy::random_expired_deploy(rng)
                }
                _ => Deploy::random_with_valid_session_package_by_name(rng),
            })
        }
        TransactionCategory::Standard => Transaction::V1(TransactionV1::random_standard(
            rng,
            strict_timestamp,
            with_ttl,
        )),
        TransactionCategory::InstallUpgrade => Transaction::V1(
            TransactionV1::random_install_upgrade(rng, strict_timestamp, with_ttl),
        ),
        TransactionCategory::Staking => Transaction::V1(TransactionV1::random_staking(
            rng,
            strict_timestamp,
            with_ttl,
        )),
    }
}

fn create_invalid_transactions(
    rng: &mut TestRng,
    transaction_category: &TransactionCategory,
    size: usize,
) -> Vec<Transaction> {
    (0..size)
        .map(|_| {
            let mut transaction = create_valid_transaction(rng, transaction_category, None, None);
            transaction.invalidate();
            transaction
        })
        .collect()
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

const fn all_categories() -> &'static [TransactionCategory] {
    &[
        TransactionCategory::InstallUpgrade,
        TransactionCategory::Staking,
        TransactionCategory::Standard,
        TransactionCategory::StandardLegacy,
        TransactionCategory::Transfer,
        TransactionCategory::TransferLegacy,
    ]
}

#[test]
fn register_transaction_and_check_size() {
    let mut rng = TestRng::new();

    for category in all_categories() {
        let mut transaction_buffer = TransactionBuffer::new(
            TransactionConfig::default(),
            Config::default(),
            &Registry::new(),
        )
        .unwrap();

        // Try to register valid transactions
        let num_valid_transactions: usize = rng.gen_range(50..500);
        let valid_transactions: Vec<_> = (0..num_valid_transactions)
            .map(|_| create_valid_transaction(&mut rng, category, None, None))
            .collect();
        valid_transactions
            .iter()
            .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
        assert_container_sizes(&transaction_buffer, valid_transactions.len(), 0, 0);

        // Try to register invalid transactions
        let num_invalid_transactions: usize = rng.gen_range(10..100);
        let invalid_transactions =
            create_invalid_transactions(&mut rng, category, num_invalid_transactions);
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
}

#[test]
fn register_block_with_valid_transactions() {
    let mut rng = TestRng::new();

    for category in all_categories() {
        let mut transaction_buffer = TransactionBuffer::new(
            TransactionConfig::default(),
            Config::default(),
            &Registry::new(),
        )
        .unwrap();

        let txns: Vec<_> = (0..10)
            .map(|_| create_valid_transaction(&mut rng, category, None, None))
            .collect();
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
        assert_container_sizes(&transaction_buffer, txns.len(), txns.len(), 0);
    }
}

#[test]
fn register_finalized_block_with_valid_transactions() {
    let mut rng = TestRng::new();

    for category in all_categories() {
        let mut transaction_buffer = TransactionBuffer::new(
            TransactionConfig::default(),
            Config::default(),
            &Registry::new(),
        )
        .unwrap();

        let txns: Vec<_> = (0..10)
            .map(|_| create_valid_transaction(&mut rng, category, None, None))
            .collect();
        let block = FinalizedBlock::random(&mut rng, &txns);

        transaction_buffer.register_block_finalized(&block);
        assert_container_sizes(&transaction_buffer, txns.len(), txns.len(), 0);
    }
}

#[test]
fn get_proposable_transactions() {
    let mut rng = TestRng::new();

    for category in all_categories() {
        let mut transaction_buffer = TransactionBuffer::new(
            TransactionConfig::default(),
            Config::default(),
            &Registry::new(),
        )
        .unwrap();

        transaction_buffer.prices.insert(EraId::new(1), 1u8);

        // populate transaction buffer with some transactions
        let transactions: Vec<_> = (0..50)
            .map(|_| create_valid_transaction(&mut rng, category, None, None))
            .collect();
        transactions
            .iter()
            .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
        assert_container_sizes(&transaction_buffer, transactions.len(), 0, 0);

        // Create a block with some transactions and register it with the transaction_buffer
        let block_transactions: Vec<_> = (0..10)
            .map(|_| create_valid_transaction(&mut rng, category, None, None))
            .collect();
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
        let proposable = transaction_buffer.proposable(1u8);
        assert_eq!(proposable.len(), transactions.len());
        let proposable_transaction_hashes: HashSet<_> = proposable
            .iter()
            .map(|(th, _)| th.transaction_hash())
            .collect();
        for transaction in transactions.iter() {
            assert!(proposable_transaction_hashes.contains(&transaction.hash()));
        }

        // Get an appendable block. This should put the transactions on hold.
        let appendable_block = transaction_buffer.appendable_block(Timestamp::now(), EraId::new(1));
        assert_eq!(transaction_buffer.hold.len(), 1);
        assert_container_sizes(
            &transaction_buffer,
            transactions.len() + block_transactions.len(),
            block_transactions.len(),
            appendable_block.transaction_and_transfer_set().len(),
        );

        // Check that held blocks are not proposable
        let proposable = transaction_buffer.proposable(1u8);
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
}

#[test]
fn get_appendable_block_when_transfers_are_of_one_category() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_transfer_count: 200,
        block_max_staking_count: 0,
        block_max_install_upgrade_count: 0,
        block_max_standard_count: 10,
        block_max_approval_count: 210,
        ..Default::default()
    };
    for category in &[
        TransactionCategory::TransferLegacy,
        TransactionCategory::Transfer,
    ] {
        let mut transaction_buffer =
            TransactionBuffer::new(transaction_config, Config::default(), &Registry::new())
                .unwrap();
        transaction_buffer.prices.insert(EraId::new(1), 1u8);
        get_appendable_block(
            &mut rng,
            &mut transaction_buffer,
            std::iter::repeat_with(|| category),
            transaction_config.block_max_transfer_count as usize,
        );
    }
}

#[test]
fn get_appendable_block_when_transfers_are_both_legacy_and_v1() {
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

    transaction_buffer.prices.insert(EraId::new(1), 1u8);

    get_appendable_block(
        &mut rng,
        &mut transaction_buffer,
        [
            TransactionCategory::TransferLegacy,
            TransactionCategory::Transfer,
        ]
        .iter()
        .cycle(),
        transaction_config.block_max_transfer_count as usize,
    );
}

#[test]
fn get_appendable_block_when_standards_are_of_one_category() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_transfer_count: 200,
        block_max_staking_count: 0,
        block_max_install_upgrade_count: 0,
        block_max_standard_count: 10,
        block_max_approval_count: 210,
        ..Default::default()
    };
    for category in &[
        TransactionCategory::Standard,
        TransactionCategory::StandardLegacy,
    ] {
        let mut transaction_buffer =
            TransactionBuffer::new(transaction_config, Config::default(), &Registry::new())
                .unwrap();

        transaction_buffer.prices.insert(EraId::new(1), 1u8);
        get_appendable_block(
            &mut rng,
            &mut transaction_buffer,
            std::iter::repeat_with(|| category),
            transaction_config.block_max_standard_count as usize,
        );
    }
}

#[test]
fn get_appendable_block_when_standards_are_both_legacy_and_v1() {
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

    transaction_buffer.prices.insert(EraId::new(1), 1u8);
    get_appendable_block(
        &mut rng,
        &mut transaction_buffer,
        [
            TransactionCategory::StandardLegacy,
            TransactionCategory::Standard,
        ]
        .iter()
        .cycle(),
        transaction_config.block_max_standard_count as usize,
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

    transaction_buffer.prices.insert(EraId::new(1), 1u8);

    // try to register valid transactions
    let num_valid_transactions: usize = rng.gen_range(50..500);
    let category = TransactionCategory::random(&mut rng);
    let valid_transactions: Vec<_> = (0..num_valid_transactions)
        .map(|_| create_valid_transaction(&mut rng, &category, None, None))
        .collect();
    valid_transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(&transaction_buffer, valid_transactions.len(), 0, 0);

    // register a block with transactions
    let category = TransactionCategory::random(&mut rng);
    let block_transaction: Vec<_> = (0..5)
        .map(|_| create_valid_transaction(&mut rng, &category, None, None))
        .collect();
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
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now(), EraId::new(1));
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
async fn expire_transactions_and_check_announcement_when_transactions_are_of_one_category() {
    let mut rng = TestRng::new();

    for category in all_categories() {
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
        let expired_transactions: Vec<_> = (0..num_transactions)
            .map(|_| create_valid_transaction(&mut rng, category, Some(past_timestamp), Some(ttl)))
            .collect();

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
        let transactions: Vec<_> = (0..num_transactions)
            .map(|_| create_valid_transaction(&mut rng, category, None, None))
            .collect();
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
}

#[tokio::test]
async fn expire_transactions_and_check_announcement_when_transactions_are_of_random_categories() {
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
    let expired_transactions: Vec<_> = (0..num_transactions)
        .map(|_| {
            let random_category = all_categories().choose(&mut rng).unwrap();
            create_valid_transaction(&mut rng, random_category, Some(past_timestamp), Some(ttl))
        })
        .collect();

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
    let transactions: Vec<_> = (0..num_transactions)
        .map(|_| {
            let random_category = all_categories().choose(&mut rng).unwrap();
            create_valid_transaction(&mut rng, random_category, None, None)
        })
        .collect();
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
