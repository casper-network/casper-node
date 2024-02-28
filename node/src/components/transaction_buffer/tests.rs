use prometheus::Registry;
use rand::{seq::SliceRandom, Rng};

use casper_types::{
    testing::TestRng, Deploy, EraId, TestBlockBuilder, TimeDiff, Transaction, TransactionV1,
};

use super::*;
use crate::{
    components::tests::TransactionCategory,
    effect::announcements::TransactionBufferAnnouncement::{self, TransactionsExpired},
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    types::FinalizedBlock,
    utils,
};

fn get_appendable_block(
    rng: &mut TestRng,
    transaction_buffer: &mut TransactionBuffer,
    categories: impl Iterator<Item = &'static TransactionCategory>,
    transaction_limit: usize,
) {
    let transactions: Vec<_> = categories
        .take(transaction_limit)
        .into_iter()
        .map(|category| create_valid_transaction(rng, category, None, None))
        .collect();
    transactions
        .iter()
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));
    assert_container_sizes(transaction_buffer, transactions.len(), 0, 0);

    // now check how many transfers were added in the block; should not exceed the config limits.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert!(appendable_block.transaction_hashes().len() <= transaction_limit);
    assert_eq!(transaction_buffer.hold.len(), 1);
    assert_container_sizes(
        transaction_buffer,
        transactions.len(),
        0,
        appendable_block.transaction_hashes().len(),
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
            appendable_block.transaction_hashes().len(),
        );

        // Check that held blocks are not proposable
        let proposable = transaction_buffer.proposable();
        assert_eq!(
            proposable.len(),
            transactions.len() - appendable_block.transaction_hashes().len()
        );
        for transaction in proposable.iter() {
            assert!(!appendable_block
                .transaction_hashes()
                .contains(&transaction.0.transaction_hash()));
        }
    }
}

#[test]
fn get_appendable_block_when_transfers_are_of_one_category() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_mint_count: 200,
        block_max_auction_count: 0,
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

        get_appendable_block(
            &mut rng,
            &mut transaction_buffer,
            std::iter::repeat_with(|| category),
            transaction_config.block_max_mint_count as usize + 50,
        );
    }
}

#[test]
fn get_appendable_block_when_transfers_are_both_legacy_and_v1() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_mint_count: 200,
        block_max_auction_count: 0,
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
        [
            TransactionCategory::TransferLegacy,
            TransactionCategory::Transfer,
        ]
        .iter()
        .cycle(),
        transaction_config.block_max_mint_count as usize + 50,
    );
}

#[test]
fn get_appendable_block_when_standards_are_of_one_category() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_mint_count: 200,
        block_max_auction_count: 0,
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
        get_appendable_block(
            &mut rng,
            &mut transaction_buffer,
            std::iter::repeat_with(|| category),
            transaction_config.block_max_standard_count as usize + 50,
        );
    }
}

#[test]
fn get_appendable_block_when_standards_are_both_legacy_and_v1() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_mint_count: 200,
        block_max_auction_count: 0,
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
        [
            TransactionCategory::StandardLegacy,
            TransactionCategory::Standard,
        ]
        .iter()
        .cycle(),
        transaction_config.block_max_standard_count as usize + 5,
    );
}

#[test]
fn block_fully_saturated() {
    let mut rng = TestRng::new();

    let max_transfers = rng.gen_range(0..20);
    let max_staking = rng.gen_range(0..20);
    let max_install_upgrade = rng.gen_range(0..20);
    let max_standard = rng.gen_range(0..20);

    let total_allowed = max_transfers + max_staking + max_install_upgrade + max_standard;

    let transaction_config = TransactionConfig {
        block_max_mint_count: max_transfers,
        block_max_auction_count: max_staking,
        block_max_install_upgrade_count: max_install_upgrade,
        block_max_standard_count: max_standard,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut transaction_buffer =
        TransactionBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();

    // Try to register 10 more transactions per each category as allowed by the config.
    let (transfers, stakings, install_upgrades, standards) = generate_and_register_transactions(
        &mut transaction_buffer,
        max_transfers + 10,
        max_staking + 10,
        max_install_upgrade + 10,
        max_standard + 10,
        &mut rng,
    );
    let (transfers_hashes, stakings_hashes, install_upgrades_hashes, standards_hashes) = (
        transfers
            .iter()
            .map(|transaction| transaction.hash())
            .collect_vec(),
        stakings
            .iter()
            .map(|transaction| transaction.hash())
            .collect_vec(),
        install_upgrades
            .iter()
            .map(|transaction| transaction.hash())
            .collect_vec(),
        standards
            .iter()
            .map(|transaction| transaction.hash())
            .collect_vec(),
    );

    // Check that we really generated the required number of transactions.
    assert_eq!(
        transfers.len() + stakings.len() + install_upgrades.len() + standards.len(),
        total_allowed as usize + 10 * 4
    );

    // Ensure that only 'total_allowed' transactions are proposed.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert_eq!(
        appendable_block.transaction_hashes().len(),
        total_allowed as usize
    );

    // Assert the number of proposed transaction types, block should be fully saturated.
    let mut proposed_transfers = 0;
    let mut proposed_stakings = 0;
    let mut proposed_install_upgrades = 0;
    let mut proposed_standards = 0;
    appendable_block
        .transaction_hashes()
        .iter()
        .for_each(|transaction_hash| {
            if transfers_hashes.contains(transaction_hash) {
                proposed_transfers += 1;
            } else if stakings_hashes.contains(transaction_hash) {
                proposed_stakings += 1;
            } else if install_upgrades_hashes.contains(transaction_hash) {
                proposed_install_upgrades += 1;
            } else if standards_hashes.contains(transaction_hash) {
                proposed_standards += 1;
            }
        });
    assert_eq!(proposed_transfers, max_transfers);
    assert_eq!(proposed_stakings, max_staking);
    assert_eq!(proposed_install_upgrades, max_install_upgrade);
    assert_eq!(proposed_standards, max_standard);
}

#[test]
fn block_not_fully_saturated() {
    let mut rng = TestRng::new();

    const MIN_COUNT: u32 = 10;

    let max_transfers = rng.gen_range(MIN_COUNT..20);
    let max_staking = rng.gen_range(MIN_COUNT..20);
    let max_install_upgrade = rng.gen_range(MIN_COUNT..20);
    let max_standard = rng.gen_range(MIN_COUNT..20);

    let total_allowed = max_transfers + max_staking + max_install_upgrade + max_standard;

    let transaction_config = TransactionConfig {
        block_max_mint_count: max_transfers,
        block_max_auction_count: max_staking,
        block_max_install_upgrade_count: max_install_upgrade,
        block_max_standard_count: max_standard,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut transaction_buffer =
        TransactionBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();

    // Try to register less than max capacity per each category as allowed by the config.
    let actual_transfer_count = rng.gen_range(0..MIN_COUNT - 1);
    let actual_stakings_count = rng.gen_range(0..MIN_COUNT - 1);
    let actual_install_upgrade_count = rng.gen_range(0..MIN_COUNT - 1);
    let actual_standard_count = rng.gen_range(0..MIN_COUNT - 1);
    let (transfers, stakings, install_upgrades, standards) = generate_and_register_transactions(
        &mut transaction_buffer,
        actual_transfer_count,
        actual_stakings_count,
        actual_install_upgrade_count,
        actual_standard_count,
        &mut rng,
    );
    let (transfers_hashes, stakings_hashes, install_upgrades_hashes, standards_hashes) = (
        transfers
            .iter()
            .map(|transaction| transaction.hash())
            .collect_vec(),
        stakings
            .iter()
            .map(|transaction| transaction.hash())
            .collect_vec(),
        install_upgrades
            .iter()
            .map(|transaction| transaction.hash())
            .collect_vec(),
        standards
            .iter()
            .map(|transaction| transaction.hash())
            .collect_vec(),
    );

    // Check that we really generated the required number of transactions.
    assert_eq!(
        transfers.len() + stakings.len() + install_upgrades.len() + standards.len(),
        actual_transfer_count as usize
            + actual_stakings_count as usize
            + actual_install_upgrade_count as usize
            + actual_standard_count as usize
    );

    // Ensure that not more than 'total_allowed' transactions are proposed.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert!(appendable_block.transaction_hashes().len() <= total_allowed as usize);

    // Assert the number of proposed transaction types, block should not be fully saturated.
    let mut proposed_transfers = 0;
    let mut proposed_stakings = 0;
    let mut proposed_install_upgrades = 0;
    let mut proposed_standards = 0;
    appendable_block
        .transaction_hashes()
        .iter()
        .for_each(|transaction_hash| {
            if transfers_hashes.contains(transaction_hash) {
                proposed_transfers += 1;
            } else if stakings_hashes.contains(transaction_hash) {
                proposed_stakings += 1;
            } else if install_upgrades_hashes.contains(transaction_hash) {
                proposed_install_upgrades += 1;
            } else if standards_hashes.contains(transaction_hash) {
                proposed_standards += 1;
            }
        });
    assert_eq!(proposed_transfers, actual_transfer_count);
    assert_eq!(proposed_stakings, actual_stakings_count);
    assert_eq!(proposed_install_upgrades, actual_install_upgrade_count);
    assert_eq!(proposed_standards, actual_standard_count);
}

#[test]
fn excess_transactions_do_not_sneak_into_transfer_bucket() {
    let mut rng = TestRng::new();

    const MAX: u32 = 20;

    let max_transfers = rng.gen_range(2..MAX);
    let max_staking = rng.gen_range(2..MAX);
    let max_install_upgrade = rng.gen_range(2..MAX);
    let max_standard = rng.gen_range(2..MAX);

    let total_allowed = max_transfers + max_staking + max_install_upgrade + max_standard;

    let transaction_config = TransactionConfig {
        block_max_mint_count: max_transfers,
        block_max_auction_count: max_staking,
        block_max_install_upgrade_count: max_install_upgrade,
        block_max_standard_count: max_standard,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut transaction_buffer =
        TransactionBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();

    // Saturate all buckets but transfers.
    let (transfers, _, _, _) = generate_and_register_transactions(
        &mut transaction_buffer,
        max_transfers - 1,
        MAX * 3,
        MAX * 3,
        MAX * 3,
        &mut rng,
    );
    let hashes_in_non_saturated_bucket: Vec<_> = transfers
        .iter()
        .map(|transaction| transaction.hash())
        .collect();

    // Ensure that only 'total_allowed - 1' transactions are proposed, since a single place int the
    // "transfers" bucket is still available.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert_eq!(
        appendable_block.transaction_hashes().len(),
        total_allowed as usize - 1
    );

    // Ensure, that it is indeed the "transfers" bucket that is not fully saturated.
    assert_bucket(
        appendable_block,
        &hashes_in_non_saturated_bucket,
        max_transfers - 1,
    );
}

#[test]
fn excess_transactions_do_not_sneak_into_staking_bucket() {
    let mut rng = TestRng::new();

    const MAX: u32 = 20;

    let max_transfers = rng.gen_range(2..MAX);
    let max_staking = rng.gen_range(2..MAX);
    let max_install_upgrade = rng.gen_range(2..MAX);
    let max_standard = rng.gen_range(2..MAX);

    let total_allowed = max_transfers + max_staking + max_install_upgrade + max_standard;

    let transaction_config = TransactionConfig {
        block_max_mint_count: max_transfers,
        block_max_auction_count: max_staking,
        block_max_install_upgrade_count: max_install_upgrade,
        block_max_standard_count: max_standard,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut transaction_buffer =
        TransactionBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();

    // Saturate all buckets but stakings.
    let (_, stakings, _, _) = generate_and_register_transactions(
        &mut transaction_buffer,
        MAX * 3,
        max_staking - 1,
        MAX * 3,
        MAX * 3,
        &mut rng,
    );
    let hashes_in_non_saturated_bucket: Vec<_> = stakings
        .iter()
        .map(|transaction| transaction.hash())
        .collect();

    // Ensure that only 'total_allowed - 1' transactions are proposed, since a single place int the
    // "stakings" bucket is still available.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert_eq!(
        appendable_block.transaction_hashes().len(),
        total_allowed as usize - 1
    );

    // Ensure, that it is indeed the "stakings" bucket that is not fully saturated.
    assert_bucket(
        appendable_block,
        &hashes_in_non_saturated_bucket,
        max_staking - 1,
    );
}

#[test]
fn excess_transactions_do_not_sneak_into_install_upgrades_bucket() {
    let mut rng = TestRng::new();

    const MAX: u32 = 20;

    let max_transfers = rng.gen_range(2..MAX);
    let max_staking = rng.gen_range(2..MAX);
    let max_install_upgrade = rng.gen_range(2..MAX);
    let max_standard = rng.gen_range(2..MAX);

    let total_allowed = max_transfers + max_staking + max_install_upgrade + max_standard;

    let transaction_config = TransactionConfig {
        block_max_mint_count: max_transfers,
        block_max_auction_count: max_staking,
        block_max_install_upgrade_count: max_install_upgrade,
        block_max_standard_count: max_standard,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut transaction_buffer =
        TransactionBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();

    // Saturate all buckets but install_upgrades.
    let (_, _, install_upgrades, _) = generate_and_register_transactions(
        &mut transaction_buffer,
        MAX * 3,
        MAX * 3,
        max_install_upgrade - 1,
        MAX * 3,
        &mut rng,
    );
    let hashes_in_non_saturated_bucket: Vec<_> = install_upgrades
        .iter()
        .map(|transaction| transaction.hash())
        .collect();

    // Ensure that only 'total_allowed - 1' transactions are proposed, since a single place int the
    // "install_upgrades" bucket is still available.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert_eq!(
        appendable_block.transaction_hashes().len(),
        total_allowed as usize - 1
    );

    // Ensure, that it is indeed the "install_upgrades" bucket that is not fully saturated.
    assert_bucket(
        appendable_block,
        &hashes_in_non_saturated_bucket,
        max_install_upgrade - 1,
    );
}

#[test]
fn excess_transactions_do_not_sneak_into_standards_bucket() {
    let mut rng = TestRng::new();

    const MAX: u32 = 20;

    let max_transfers = rng.gen_range(2..MAX);
    let max_staking = rng.gen_range(2..MAX);
    let max_install_upgrade = rng.gen_range(2..MAX);
    let max_standard = rng.gen_range(2..MAX);

    let total_allowed = max_transfers + max_staking + max_install_upgrade + max_standard;

    let transaction_config = TransactionConfig {
        block_max_mint_count: max_transfers,
        block_max_auction_count: max_staking,
        block_max_install_upgrade_count: max_install_upgrade,
        block_max_standard_count: max_standard,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut transaction_buffer =
        TransactionBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();

    // Saturate all buckets but standards.
    let (_, _, _, standards) = generate_and_register_transactions(
        &mut transaction_buffer,
        MAX * 3,
        MAX * 3,
        MAX * 3,
        max_standard - 1,
        &mut rng,
    );
    let hashes_in_non_saturated_bucket: Vec<_> = standards
        .iter()
        .map(|transaction| transaction.hash())
        .collect();

    // Ensure that only 'total_allowed - 1' transactions are proposed, since a single place int the
    // "standards" bucket is still available.
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert_eq!(
        appendable_block.transaction_hashes().len(),
        total_allowed as usize - 1
    );

    // Ensure, that it is indeed the "standards" bucket that is not fully saturated.
    assert_bucket(
        appendable_block,
        &hashes_in_non_saturated_bucket,
        max_standard - 1,
    );
}

fn assert_bucket(
    appendable_block: AppendableBlock,
    hashes_in_non_saturated_bucket: &[TransactionHash],
    expected: u32,
) {
    let mut proposed = 0;
    appendable_block
        .transaction_hashes()
        .iter()
        .for_each(|transaction_hash| {
            if hashes_in_non_saturated_bucket.contains(transaction_hash) {
                proposed += 1;
            }
        });
    assert_eq!(proposed, expected);
}

fn generate_and_register_transactions(
    transaction_buffer: &mut TransactionBuffer,
    transfer_count: u32,
    stakings_count: u32,
    install_upgrade_count: u32,
    standard_count: u32,
    rng: &mut TestRng,
) -> (
    Vec<Transaction>,
    Vec<Transaction>,
    Vec<Transaction>,
    Vec<Transaction>,
) {
    let transfers: Vec<_> = (0..transfer_count)
        .map(|_| {
            let category = if rng.gen() {
                TransactionCategory::Transfer
            } else {
                TransactionCategory::TransferLegacy
            };
            create_valid_transaction(rng, &category, None, None)
        })
        .collect();
    let stakings: Vec<_> = (0..stakings_count)
        .map(|_| create_valid_transaction(rng, &TransactionCategory::Staking, None, None))
        .collect();
    let installs_upgrades: Vec<_> = (0..install_upgrade_count)
        .map(|_| create_valid_transaction(rng, &TransactionCategory::InstallUpgrade, None, None))
        .collect();
    let standards: Vec<_> = (0..standard_count)
        .map(|_| {
            let category = if rng.gen() {
                TransactionCategory::Standard
            } else {
                TransactionCategory::StandardLegacy
            };
            create_valid_transaction(rng, &category, None, None)
        })
        .collect();
    transfers
        .iter()
        .chain(
            stakings
                .iter()
                .chain(installs_upgrades.iter().chain(standards.iter())),
        )
        .for_each(|transaction| transaction_buffer.register_transaction(transaction.clone()));

    (transfers, stakings, installs_upgrades, standards)
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
    let appendable_block = transaction_buffer.appendable_block(Timestamp::now());
    assert_eq!(transaction_buffer.hold.len(), 1);
    assert_container_sizes(
        &transaction_buffer,
        block_transaction.len() + valid_transactions.len(),
        block_transaction.len(),
        appendable_block.transaction_hashes().len(),
    );

    // try to register held transactions again.
    let mut held_transactions = valid_transactions
        .iter()
        .cloned()
        .filter(|transaction| {
            appendable_block
                .transaction_hashes()
                .contains(&transaction.hash())
        })
        .peekable();
    assert!(held_transactions.peek().is_some());
    held_transactions.for_each(|transaction| transaction_buffer.register_transaction(transaction));
    assert_container_sizes(
        &transaction_buffer,
        block_transaction.len() + valid_transactions.len(),
        block_transaction.len(),
        appendable_block.transaction_hashes().len(),
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
