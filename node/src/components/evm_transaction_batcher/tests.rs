use std::{sync::Arc, time::Duration};

use alloy_consensus::{SignableTransaction, TxEnvelope, TxLegacy};
use alloy_eips::Encodable2718;
use alloy_primitives::{Bytes as AlloyBytes, Signature, TxKind, U256};
use k256::ecdsa::{signature::hazmat::PrehashSigner, SigningKey};

use casper_types::{
    testing::TestRng, Deploy, EvmTransaction, TimeDiff, Timestamp, Transaction, TransactionId,
};

use derive_more::From;

use super::{split_into_runs, Config, EvmTransactionBatcher};
use crate::{
    effect::{requests::FetcherRequest, EffectBuilder},
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    types::{
        check_evm_transaction_batch_follows, BatchedTransaction, EvmTransactionBatch,
        EvmTransactionBatchError, EvmTransactionBatchId, NodeId,
        EVM_TRANSACTION_BATCH_SIZE_CEILING,
    },
};

/// The only event the batcher's fetching code schedules.
#[derive(Debug, From)]
enum TestEvent {
    BatchedTransactionFetcherRequest(#[allow(dead_code)] FetcherRequest<BatchedTransaction>),
}

fn effect_builder() -> EffectBuilder<TestEvent> {
    let scheduler: &'static Scheduler<TestEvent> =
        Box::leak(Box::new(Scheduler::new(QueueKind::weights(), None)));
    EffectBuilder::new(EventQueueHandle::without_shutdown(scheduler))
}

fn batch_id(signing_key_byte: u8) -> EvmTransactionBatchId {
    EvmTransactionBatch::new(vec![
        evm_transaction(signing_key_byte, 0),
        evm_transaction(signing_key_byte, 1),
    ])
    .expect("should be a valid batch")
    .id()
}

fn batcher(max_batch_size: u32, max_in_flight_batch_fetches: u32) -> EvmTransactionBatcher {
    EvmTransactionBatcher::new(
        Config {
            max_batch_size,
            max_in_flight_batch_fetches,
        },
        Duration::from_secs(100),
    )
}

fn evm_transaction(signing_key_byte: u8, nonce: u64) -> Transaction {
    let transaction = TxLegacy {
        chain_id: Some(1),
        nonce,
        gas_price: 1,
        gas_limit: 21_000,
        to: TxKind::Call([1; 20].into()),
        value: U256::ZERO,
        input: AlloyBytes::new(),
    };
    let signing_key = SigningKey::from_slice(&[signing_key_byte; 32])
        .expect("test EVM private key should be valid");
    let (signature, recovery_id) = signing_key
        .sign_prehash(transaction.signature_hash().as_ref())
        .expect("test EVM transaction signing should succeed");
    let signed = transaction.into_signed(Signature::from((signature, recovery_id)));
    let evm_transaction = EvmTransaction::from_signed_rlp(
        TxEnvelope::from(signed).encoded_2718(),
        Timestamp::now(),
        TimeDiff::from_seconds(300),
    )
    .expect("test EVM transaction should decode");
    Transaction::from(evm_transaction)
}

fn nonces(transactions: &[Arc<Transaction>]) -> Vec<u64> {
    transactions
        .iter()
        .map(|transaction| transaction.as_evm().unwrap().nonce())
        .collect()
}

#[test]
fn should_split_sender_transactions_into_consecutive_runs() {
    let transactions = [7, 3, 5, 4, 3, 8]
        .into_iter()
        .map(|nonce| Arc::new(evm_transaction(0x11, nonce)))
        .collect();

    let runs: Vec<_> = split_into_runs(transactions, 10)
        .iter()
        .map(|run| nonces(run))
        .collect();

    assert_eq!(runs, vec![vec![3], vec![3, 4, 5], vec![7, 8]]);
}

#[test]
fn should_cap_runs_at_max_batch_size() {
    const MAX_BATCH_SIZE: usize = 4;
    let transactions = (0..MAX_BATCH_SIZE as u64 * 2 + 3)
        .map(|nonce| Arc::new(evm_transaction(0x11, nonce)))
        .collect();

    let run_lengths: Vec<_> = split_into_runs(transactions, MAX_BATCH_SIZE)
        .iter()
        .map(Vec::len)
        .collect();

    assert_eq!(run_lengths, vec![MAX_BATCH_SIZE, MAX_BATCH_SIZE, 3]);
}

#[test]
fn should_accept_consecutive_batch_from_one_sender() {
    let batch =
        EvmTransactionBatch::new((3..6).map(|nonce| evm_transaction(0x11, nonce)).collect())
            .expect("should be a valid batch");

    assert_eq!(batch.id().transaction_ids().len(), 3);
}

#[test]
fn should_reject_invalid_batches() {
    assert_eq!(
        EvmTransactionBatch::new(vec![evm_transaction(0x11, 0)]),
        Err(EvmTransactionBatchError::InvalidSize(1))
    );
    assert_eq!(
        EvmTransactionBatch::new(
            (0..EVM_TRANSACTION_BATCH_SIZE_CEILING as u64 + 1)
                .map(|nonce| evm_transaction(0x11, nonce))
                .collect()
        ),
        Err(EvmTransactionBatchError::InvalidSize(
            EVM_TRANSACTION_BATCH_SIZE_CEILING + 1
        ))
    );
    assert_eq!(
        EvmTransactionBatch::new(vec![evm_transaction(0x11, 0), evm_transaction(0x22, 1)]),
        Err(EvmTransactionBatchError::MixedSenders)
    );
    assert_eq!(
        EvmTransactionBatch::new(vec![evm_transaction(0x11, 0), evm_transaction(0x11, 2)]),
        Err(EvmTransactionBatchError::NonConsecutiveNonces {
            previous: 0,
            next: 2
        })
    );
    assert_eq!(
        EvmTransactionBatch::new(vec![evm_transaction(0x11, 1), evm_transaction(0x11, 0)]),
        Err(EvmTransactionBatchError::NonConsecutiveNonces {
            previous: 1,
            next: 0
        })
    );
    let deploy = Transaction::from(Deploy::random(&mut TestRng::new()));
    assert_eq!(
        EvmTransactionBatch::new(vec![evm_transaction(0x11, 0), deploy.clone()]),
        Err(EvmTransactionBatchError::NotEvm(deploy.hash()))
    );
}

#[test]
fn should_reject_oversized_batch_id_on_deserialization() {
    let transaction_ids: Vec<TransactionId> = (0..EVM_TRANSACTION_BATCH_SIZE_CEILING as u64 + 1)
        .map(|nonce| evm_transaction(0x11, nonce).compute_id())
        .collect();

    let serialized_id = bincode::serialize(&transaction_ids).unwrap();
    assert!(bincode::deserialize::<EvmTransactionBatchId>(&serialized_id).is_err());

    let within_limit = &transaction_ids[..EVM_TRANSACTION_BATCH_SIZE_CEILING];
    let serialized_id = bincode::serialize(within_limit).unwrap();
    assert!(bincode::deserialize::<EvmTransactionBatchId>(&serialized_id).is_ok());
}

#[test]
fn should_check_each_fetched_transaction_against_the_previous_one() {
    let first = evm_transaction(0x11, 3);

    assert_eq!(check_evm_transaction_batch_follows(None, &first), Ok(()));
    assert_eq!(
        check_evm_transaction_batch_follows(Some(&first), &evm_transaction(0x11, 4)),
        Ok(())
    );
    assert_eq!(
        check_evm_transaction_batch_follows(Some(&first), &evm_transaction(0x11, 5)),
        Err(EvmTransactionBatchError::NonConsecutiveNonces {
            previous: 3,
            next: 5
        })
    );
    assert_eq!(
        check_evm_transaction_batch_follows(Some(&first), &evm_transaction(0x22, 4)),
        Err(EvmTransactionBatchError::MixedSenders)
    );
    let deploy = Transaction::from(Deploy::random(&mut TestRng::new()));
    assert_eq!(
        check_evm_transaction_batch_follows(None, &deploy),
        Err(EvmTransactionBatchError::NotEvm(deploy.hash()))
    );
}

#[test]
fn should_serialize_batched_transaction_as_plain_transaction() {
    let transaction = evm_transaction(0x11, 0);

    let serialized = bincode::serialize(&BatchedTransaction::new(transaction.clone())).unwrap();

    assert_eq!(serialized, bincode::serialize(&transaction).unwrap());
    assert_eq!(
        bincode::deserialize::<BatchedTransaction>(&serialized)
            .unwrap()
            .into_inner(),
        transaction
    );
}

#[test]
fn should_limit_in_flight_batch_fetches() {
    let mut rng = TestRng::new();
    let effect_builder = effect_builder();
    let mut batcher = batcher(10, 2);
    let peer = NodeId::random(&mut rng);

    for signing_key_byte in [0x11, 0x22, 0x33] {
        let _ = batcher.handle_batch_announced(effect_builder, batch_id(signing_key_byte), peer);
    }
    assert_eq!(batcher.acquiring.len(), 2);
    assert!(!batcher.acquiring.contains_key(&batch_id(0x33)));

    // Another holder of a batch already being fetched is still recorded.
    let other_peer = NodeId::random(&mut rng);
    let _ = batcher.handle_batch_announced(effect_builder, batch_id(0x11), other_peer);
    assert_eq!(
        batcher.acquiring[&batch_id(0x11)].holders,
        vec![peer, other_peer]
    );

    // Once a fetch finishes, a new batch can be fetched.
    let acquisition_id = batcher.acquiring[&batch_id(0x11)].acquisition_id;
    let _ = batcher.handle_acquisition_deadline_elapsed(batch_id(0x11), acquisition_id);
    let _ = batcher.handle_batch_announced(effect_builder, batch_id(0x33), peer);
    assert!(batcher.acquiring.contains_key(&batch_id(0x33)));
}

#[test]
fn should_not_limit_batch_fetches_when_configured_with_zero() {
    let mut rng = TestRng::new();
    let effect_builder = effect_builder();
    let mut batcher = batcher(10, 0);
    let peer = NodeId::random(&mut rng);

    for signing_key_byte in 1..=20 {
        let _ = batcher.handle_batch_announced(effect_builder, batch_id(signing_key_byte), peer);
    }
    assert_eq!(batcher.acquiring.len(), 20);
}

#[test]
fn should_ignore_announced_batch_larger_than_max_batch_size() {
    let mut rng = TestRng::new();
    let effect_builder = effect_builder();
    let mut batcher = batcher(2, 0);
    let peer = NodeId::random(&mut rng);
    let three_transactions =
        EvmTransactionBatch::new((0..3).map(|nonce| evm_transaction(0x11, nonce)).collect())
            .expect("should be a valid batch")
            .id();

    let _ = batcher.handle_batch_announced(effect_builder, three_transactions, peer);
    let _ = batcher.handle_batch_announced(effect_builder, batch_id(0x22), peer);

    assert_eq!(batcher.acquiring.len(), 1);
    assert!(batcher.acquiring.contains_key(&batch_id(0x22)));
}

#[test]
fn should_clamp_max_batch_size_to_supported_range() {
    let mut config = Config {
        max_batch_size: 1,
        max_in_flight_batch_fetches: 10,
    };
    assert!(config.clamp_max_batch_size().is_some());
    assert_eq!(config.max_batch_size(), 2);

    config.max_batch_size = EVM_TRANSACTION_BATCH_SIZE_CEILING as u32 + 1;
    assert!(config.clamp_max_batch_size().is_some());
    assert_eq!(config.max_batch_size(), EVM_TRANSACTION_BATCH_SIZE_CEILING);

    config.max_batch_size = 10;
    assert!(config.clamp_max_batch_size().is_none());
    assert_eq!(config.max_batch_size(), 10);
}
