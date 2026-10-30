use std::{sync::Arc, time::Duration};

use alloy_consensus::{SignableTransaction, TxEnvelope, TxLegacy};
use alloy_eips::Encodable2718;
use alloy_primitives::{Bytes as AlloyBytes, Signature, TxKind, U256};
use k256::ecdsa::{signature::hazmat::PrehashSigner, SigningKey};

use casper_types::{
    testing::TestRng, Chainspec, Deploy, EvmConfig, EvmTransaction, TimeDiff, Timestamp,
    Transaction, TransactionId, TransactionLaneDefinition,
};

use derive_more::From;

use super::{in_batch_order, Config, EvmTransactionBatcher};
use crate::{
    components::fetcher::FetchedData,
    effect::{
        requests::{AcceptTransactionRequest, FetcherRequest},
        EffectBuilder,
    },
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    types::{
        check_evm_transaction_batch_follows, evm_transaction_batch_order_key, BatchedTransaction,
        EvmTransactionBatch, EvmTransactionBatchError, EvmTransactionBatchId, NodeId,
        EVM_TRANSACTION_BATCH_SIZE_CEILING,
    },
};

/// The events the batcher's fetching and acceptance code schedules. Never stored, only dropped.
#[derive(Debug, From)]
#[allow(clippy::large_enum_variant)]
enum TestEvent {
    BatchedTransactionFetcherRequest(#[allow(dead_code)] FetcherRequest<BatchedTransaction>),
    AcceptTransactionRequest(#[allow(dead_code)] AcceptTransactionRequest),
}

/// The chain ID of the test transactions.
const CHAIN_ID: u64 = 1;
/// The largest serialized EVM transaction the test chainspec's only lane takes.
const MAX_TRANSACTION_SIZE: u64 = 4096;

/// A chainspec whose EVM config accepts the test transactions: their chain ID, a base fee equal
/// to their gas price, and one lane taking transactions of up to `MAX_TRANSACTION_SIZE` bytes.
fn chainspec() -> Arc<Chainspec> {
    Arc::new(Chainspec {
        evm_config: EvmConfig {
            enabled: true,
            chain_id: CHAIN_ID,
            base_fee: 1,
            wei_per_mote: 1,
            transaction_lanes: vec![TransactionLaneDefinition::new(
                100,
                MAX_TRANSACTION_SIZE,
                u64::MAX,
                u64::MAX,
                50,
            )],
            ..EvmConfig::default()
        },
        ..Chainspec::default()
    })
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
        chainspec(),
        Duration::from_secs(100),
    )
}

fn evm_transaction(signing_key_byte: u8, nonce: u64) -> Transaction {
    evm_transaction_with(signing_key_byte, nonce, CHAIN_ID, Vec::new())
}

fn evm_transaction_with(
    signing_key_byte: u8,
    nonce: u64,
    chain_id: u64,
    input: Vec<u8>,
) -> Transaction {
    let transaction = TxLegacy {
        chain_id: Some(chain_id),
        nonce,
        gas_price: 1,
        gas_limit: 21_000,
        to: TxKind::Call([1; 20].into()),
        value: U256::ZERO,
        input: AlloyBytes::from(input),
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
fn should_put_sender_transactions_in_one_batch_ordered_by_nonce_then_hash() {
    // A gap, a resubmission of the same transaction, and a different transaction sharing a nonce.
    let transactions = [7, 3, 5, 4, 3, 8]
        .into_iter()
        .map(|nonce| Arc::new(evm_transaction(0x11, nonce)))
        .chain([Arc::new(evm_transaction_with(0x11, 3, CHAIN_ID, vec![1]))])
        .collect();

    let ordered = in_batch_order(transactions);

    assert_eq!(nonces(&ordered), vec![3, 3, 4, 5, 7, 8]);
    assert!(ordered.windows(2).all(|pair| {
        evm_transaction_batch_order_key(&pair[0]) < evm_transaction_batch_order_key(&pair[1])
    }));
    EvmTransactionBatch::new(
        ordered
            .iter()
            .map(|transaction| (**transaction).clone())
            .collect(),
    )
    .expect("ordered transactions should form a valid batch");
}

#[test]
fn should_accept_ascending_batch_from_one_sender() {
    let batch =
        EvmTransactionBatch::new((3..6).map(|nonce| evm_transaction(0x11, nonce)).collect())
            .expect("should be a valid batch");

    assert_eq!(batch.id().transaction_ids().len(), 3);

    let batch = EvmTransactionBatch::new(
        [3, 4, 6, 7]
            .into_iter()
            .map(|nonce| evm_transaction(0x11, nonce))
            .collect(),
    )
    .expect("a nonce gap should not invalidate a batch");

    assert_eq!(batch.id().transaction_ids().len(), 4);
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
        EvmTransactionBatch::new(vec![evm_transaction(0x11, 2), evm_transaction(0x11, 2)]),
        Err(EvmTransactionBatchError::OutOfOrder {
            previous: evm_transaction(0x11, 2).hash(),
            next: evm_transaction(0x11, 2).hash(),
        })
    );
    assert_eq!(
        EvmTransactionBatch::new(vec![evm_transaction(0x11, 1), evm_transaction(0x11, 0)]),
        Err(EvmTransactionBatchError::OutOfOrder {
            previous: evm_transaction(0x11, 1).hash(),
            next: evm_transaction(0x11, 0).hash(),
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
        Ok(())
    );
    assert_eq!(
        check_evm_transaction_batch_follows(Some(&first), &evm_transaction(0x11, 2)),
        Err(EvmTransactionBatchError::OutOfOrder {
            previous: first.hash(),
            next: evm_transaction(0x11, 2).hash(),
        })
    );
    assert_eq!(
        check_evm_transaction_batch_follows(Some(&first), &first),
        Err(EvmTransactionBatchError::OutOfOrder {
            previous: first.hash(),
            next: first.hash(),
        })
    );
    // A transaction sharing a nonce follows one with a lower hash, but not one with a higher hash.
    let mut same_nonce = [
        first.clone(),
        evm_transaction_with(0x11, 3, CHAIN_ID, vec![1]),
    ];
    same_nonce.sort_by_key(|transaction| transaction.hash());
    assert_eq!(
        check_evm_transaction_batch_follows(Some(&same_nonce[0]), &same_nonce[1]),
        Ok(())
    );
    assert_eq!(
        check_evm_transaction_batch_follows(Some(&same_nonce[1]), &same_nonce[0]),
        Err(EvmTransactionBatchError::OutOfOrder {
            previous: same_nonce[1].hash(),
            next: same_nonce[0].hash(),
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

/// Hands `transactions` to the batcher one after another as if fetched from `peer` for the batch
/// currently being acquired, returning the number of effects the last one produced.
fn fetch_transactions(
    batcher: &mut EvmTransactionBatcher,
    effect_builder: EffectBuilder<TestEvent>,
    batch_id: &EvmTransactionBatchId,
    transactions: &[Transaction],
    peer: NodeId,
) -> usize {
    let acquisition_id = batcher.acquiring[batch_id].acquisition_id;
    let mut effect_count = 0;
    for transaction in transactions {
        let effects = batcher.handle_transaction_fetched(
            effect_builder,
            batch_id.clone(),
            acquisition_id,
            Ok(FetchedData::from_peer(
                BatchedTransaction::new(transaction.clone()),
                peer,
            )),
        );
        effect_count = effects.len();
    }
    effect_count
}

fn batch_id_of(transactions: &[Transaction]) -> EvmTransactionBatchId {
    EvmTransactionBatchId::try_from(
        transactions
            .iter()
            .map(Transaction::compute_id)
            .collect::<Vec<_>>(),
    )
    .expect("should be a valid batch ID")
}

#[test]
fn should_accept_batch_whose_fetched_transactions_are_valid() {
    let mut rng = TestRng::new();
    let effect_builder = effect_builder();
    let mut batcher = batcher(10, 10);
    let peer = NodeId::random(&mut rng);
    let transactions: Vec<_> = (0..3).map(|nonce| evm_transaction(0x11, nonce)).collect();
    let batch_id = batch_id_of(&transactions);

    let _ = batcher.handle_batch_announced(effect_builder, batch_id.clone(), peer);
    let effect_count =
        fetch_transactions(&mut batcher, effect_builder, &batch_id, &transactions, peer);

    // The complete batch is handed to the acceptor.
    assert_eq!(effect_count, 1);
    assert!(batcher.acquiring.is_empty());
    assert_eq!(batcher.fetched_batches(), 1);
}

#[test]
fn should_drop_whole_batch_when_a_fetched_transaction_is_invalid() {
    let invalid_second_transactions = [
        (
            "wrong chain ID",
            evm_transaction_with(0x11, 1, CHAIN_ID + 1, Vec::new()),
        ),
        (
            "too large for any EVM lane",
            evm_transaction_with(0x11, 1, CHAIN_ID, vec![0; MAX_TRANSACTION_SIZE as usize]),
        ),
        ("repeats the previous transaction", evm_transaction(0x11, 0)),
        ("from another sender", evm_transaction(0x22, 1)),
    ];

    let mut rng = TestRng::new();
    for (case, invalid) in invalid_second_transactions {
        let effect_builder = effect_builder();
        let mut batcher = batcher(10, 10);
        let peer = NodeId::random(&mut rng);
        let first = evm_transaction(0x11, 0);
        let last = evm_transaction(0x11, 2);
        let batch_id = batch_id_of(&[first.clone(), invalid.clone(), last.clone()]);

        let _ = batcher.handle_batch_announced(effect_builder, batch_id.clone(), peer);
        let acquisition_id = batcher.acquiring[&batch_id].acquisition_id;

        // The first transaction is held, and the next one requested.
        let effect_count = fetch_transactions(
            &mut batcher,
            effect_builder,
            &batch_id,
            std::slice::from_ref(&first),
            peer,
        );
        assert_eq!(effect_count, 1, "{case}");
        assert_eq!(batcher.acquiring[&batch_id].fetched, vec![first], "{case}");

        // The invalid one drops the whole batch: nothing more is fetched, nothing is passed to
        // the acceptor, and the transaction already fetched is no longer held.
        let effect_count =
            fetch_transactions(&mut batcher, effect_builder, &batch_id, &[invalid], peer);
        assert_eq!(effect_count, 0, "{case}");
        assert!(batcher.acquiring.is_empty(), "{case}");
        assert_eq!(batcher.fetched_batches(), 0, "{case}");

        // A fetch for the batch still in flight when it was dropped is ignored.
        let effects = batcher.handle_transaction_fetched(
            effect_builder,
            batch_id.clone(),
            acquisition_id,
            Ok(FetchedData::from_peer(BatchedTransaction::new(last), peer)),
        );
        assert!(effects.is_empty(), "{case}");
        assert!(batcher.acquiring.is_empty(), "{case}");

        // Announcing the batch again starts over from its first transaction.
        let _ = batcher.handle_batch_announced(effect_builder, batch_id.clone(), peer);
        assert!(batcher.acquiring[&batch_id].fetched.is_empty(), "{case}");
    }
}
