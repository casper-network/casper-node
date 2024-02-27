use std::collections::{BTreeMap, VecDeque};

use assert_matches::assert_matches;
use rand::Rng;

use casper_types::{
    global_state::TrieMerkleProof, testing::TestRng, AccessRights, CLValue, StoredValue,
    TestBlockBuilder, Transaction, URef,
};

use crate::types::ApprovalsHashes;

use super::*;

fn gen_test_transactions(rng: &mut TestRng) -> BTreeMap<TransactionHash, Transaction> {
    let num_txns = rng.gen_range(2..15);
    (0..num_txns)
        .into_iter()
        .map(|_| {
            let transaction = Transaction::random(rng);
            (transaction.hash(), transaction)
        })
        .collect()
}

fn gen_approvals_hashes<'a, I: Iterator<Item = &'a Transaction> + Clone>(
    rng: &mut TestRng,
    transactions_iter: I,
) -> ApprovalsHashes {
    let era = rng.gen_range(0..6);
    let block = TestBlockBuilder::new()
        .era(era)
        .height(era * 10 + rng.gen_range(0..10))
        .transactions(transactions_iter.clone())
        .build(rng);

    ApprovalsHashes::new_v2(
        *block.hash(),
        transactions_iter
            .map(|txn| txn.compute_approvals_hash().unwrap())
            .collect(),
        TrieMerkleProof::new(
            URef::new([255; 32], AccessRights::NONE).into(),
            StoredValue::CLValue(CLValue::from_t(()).unwrap()),
            VecDeque::new(),
        ),
    )
}

fn get_transaction_id(transaction: &Transaction) -> TransactionId {
    match transaction {
        Transaction::Deploy(deploy) => {
            TransactionId::new_deploy(*deploy.hash(), deploy.compute_approvals_hash().unwrap())
        }
        Transaction::V1(transaction_v1) => TransactionId::new_v1(
            *transaction_v1.hash(),
            transaction_v1.compute_approvals_hash().unwrap(),
        ),
    }
}

#[test]
fn dont_apply_approvals_hashes_when_acquiring_by_id() {
    let mut rng = TestRng::new();
    let test_transactions = gen_test_transactions(&mut rng);
    let approvals_hashes = gen_approvals_hashes(&mut rng, test_transactions.values());

    let mut txn_acquisition = TransactionAcquisition::ById(Acquisition::new(
        test_transactions.values().map(get_transaction_id).collect(),
        false,
    ));

    assert_matches!(
        txn_acquisition.apply_approvals_hashes(&approvals_hashes),
        Err(Error::AcquisitionByIdNotPossible)
    );
    assert_matches!(
        txn_acquisition.next_needed_transaction().unwrap(),
        TransactionIdentifier::ById(id) if test_transactions.contains_key(&id.transaction_hash())
    );
}

#[test]
fn apply_approvals_on_acquisition_by_hash_creates_correct_ids() {
    let mut rng = TestRng::new();
    let test_transactions = gen_test_transactions(&mut rng);
    let mut txn_acquisition =
        TransactionAcquisition::new_by_hash(test_transactions.keys().copied().collect(), false);

    // Generate the ApprovalsHashes for all test transactions except the last one
    let approvals_hashes = gen_approvals_hashes(
        &mut rng,
        test_transactions.values().take(test_transactions.len() - 1),
    );

    assert_matches!(
        txn_acquisition.next_needed_transaction().unwrap(),
        TransactionIdentifier::ByHash(hash) if test_transactions.contains_key(&hash)
    );
    assert!(txn_acquisition
        .apply_approvals_hashes(&approvals_hashes)
        .is_ok());

    // Now acquisition is done by id
    assert_matches!(
        txn_acquisition.next_needed_transaction().unwrap(),
        TransactionIdentifier::ById(id) if test_transactions.contains_key(&id.transaction_hash())
    );

    // Apply the transactions
    for transaction in test_transactions.values().take(test_transactions.len() - 1) {
        let acceptance = txn_acquisition.apply_transaction(get_transaction_id(transaction));
        assert_matches!(acceptance, Some(Acceptance::NeededIt));
    }

    // The last transaction was excluded from acquisition when we applied the approvals hashes so it
    // should not be needed
    assert!(!txn_acquisition.needs_transaction());

    // Try to apply the last transaction; it should not be accepted
    let last_transaction = test_transactions.values().last().unwrap();
    let last_txn_acceptance =
        txn_acquisition.apply_transaction(get_transaction_id(last_transaction));
    assert_matches!(last_txn_acceptance, None);
}

#[test]
fn apply_approvals_hashes_after_having_already_applied_transactions() {
    let mut rng = TestRng::new();
    let test_transactions = gen_test_transactions(&mut rng);
    let mut txn_acquisition =
        TransactionAcquisition::new_by_hash(test_transactions.keys().copied().collect(), false);
    let (_, first_txn) = test_transactions.first_key_value().unwrap();

    let approvals_hashes = gen_approvals_hashes(&mut rng, test_transactions.values());

    // Apply a valid transaction that was not applied before. This should succeed.
    let acceptance = txn_acquisition.apply_transaction(get_transaction_id(first_txn));
    assert_matches!(acceptance, Some(Acceptance::NeededIt));

    // Apply approvals hashes. This should fail since we have already acquired transactions by hash.
    assert_matches!(
        txn_acquisition.apply_approvals_hashes(&approvals_hashes),
        Err(Error::EncounteredNonVacantTransactionState)
    );
}

#[test]
fn partially_applied_txns_on_acquisition_by_hash_should_need_missing_txns() {
    let mut rng = TestRng::new();
    let test_transactions = gen_test_transactions(&mut rng);
    let mut txn_acquisition =
        TransactionAcquisition::new_by_hash(test_transactions.keys().copied().collect(), false);

    assert_matches!(
        txn_acquisition.next_needed_transaction().unwrap(),
        TransactionIdentifier::ByHash(hash) if test_transactions.contains_key(&hash)
    );

    // Apply all the transactions except for the last one
    for transaction in test_transactions.values().take(test_transactions.len() - 1) {
        let acceptance = txn_acquisition.apply_transaction(get_transaction_id(transaction));
        assert_matches!(acceptance, Some(Acceptance::NeededIt));
    }

    // Last transaction should be needed now
    let last_txn = test_transactions.iter().last().unwrap().1;
    assert_matches!(
        txn_acquisition.next_needed_transaction().unwrap(),
        TransactionIdentifier::ByHash(hash) if last_txn.hash() == hash
    );

    // Apply the last transaction and check the acceptance
    let last_txn_acceptance = txn_acquisition.apply_transaction(get_transaction_id(last_txn));
    assert_matches!(last_txn_acceptance, Some(Acceptance::NeededIt));

    // Try to add the last transaction again to check the acceptance
    let already_registered_acceptance =
        txn_acquisition.apply_transaction(get_transaction_id(last_txn));
    assert_matches!(already_registered_acceptance, Some(Acceptance::HadIt));
}

#[test]
fn apply_unregistered_transaction_returns_no_acceptance() {
    let mut rng = TestRng::new();
    let test_transactions = gen_test_transactions(&mut rng);
    let mut txn_acquisition =
        TransactionAcquisition::new_by_hash(test_transactions.keys().copied().collect(), false);

    let unregistered_transaction = Transaction::random(&mut rng);
    let unregistered_txn_acceptance =
        txn_acquisition.apply_transaction(get_transaction_id(&unregistered_transaction));

    // An unregistered transaction should not be accepted
    assert!(unregistered_txn_acceptance.is_none());
    let first_transaction = test_transactions.iter().next().unwrap().1;
    assert_matches!(
        txn_acquisition.next_needed_transaction().unwrap(),
        TransactionIdentifier::ByHash(hash) if first_transaction.hash() == hash
    );
}
