//! Gossips a sender's EVM transactions together.
//!
//! # Why
//!
//! If a block carries multiple evm transactions from one sender their successfull execution is dependant on the nonce sequence.
//! The proposer orders a senders transactions by nonce, but it can only
//! order the transactions it holds. Gossiping EVM transactions from one sender to different peers multiplies the probability that they will not end up in one proposal - hence making it less likely that they will execute correctly
//! If a client sends evm trnsactions this component will group them by sender and batch them by nonce maximizing the probability that they will be gossiped and proposed as one coherent batch.
//!
//! # How
//!
//! * Client side: an EVM transaction accepted from a client is held for a short window
//!   ([`BATCH_WINDOW`]), opened by the first transaction from that sender. When the window
//!   elapses, or `max_batch_size` (see [`Config`]) transactions are waiting, the sender's
//!   transactions are put in batch order, by nonce and then by hash, and gossiped as one batch;
//!   a lone transaction is gossiped as usual. The batcher does not correct the sender's input:
//!   nonce gaps and transactions sharing a nonce stay in the batch, in the order the proposer
//!   will use, and fail on execution as they would otherwise.
//! * Peer side: only a batch's ID, the ordered IDs of its transactions, is gossiped. Once a peer
//!   has announced a batch, its transactions are fetched one at a time, in the hashes order in the batch message, from the
//!   peers which announced it, and held in memory. Each is checked against the one before it (one
//!   sender, ascendning nonces) and against the chainspec (size and lane limits, chain ID, fees)
//!   as it arrives, so an invalid batch is dropped before the rest of it is fetched, and no
//!   transaction larger than an EVM lane allows is ever held. A batch with more than `max_batch_size` transactions is ignored. Nothing is stored until the whole batch has arrived: its transactions are then
//!   passed to the transaction acceptor, which stores them, in nonce order. If all of them are
//!   accepted, the batch is gossiped onwards as a batch; otherwise the accepted ones are gossiped
//!   on their own. A batch reaches the transaction buffer once the batch gossiper has finished
//!   with it, so its transactions become proposable together.
//!
//!   Fetching the transactions separately means a batch never has to fit into one network
//!   message, and a node fetches only the transactions it does not already hold. In exchange,
//!   a batch costs one request per transaction, made one after another.
//!
//! # Assumptions and limitations
//!
//! * Only transactions submitted to the same node within one window are batched. Transactions a
//!   sender spreads over several nodes, or submits further apart than the window, are gossiped
//!   separately and may not be proposed together.
//! * A run is only as good as the order the sender chose. If a sender submits nonce 4 before nonce
//!   3 and nonce 3 arrives after the window closes, they are gossiped separately and nonce 4 can
//!   be proposed, and fail, before nonce 3 is known. Sending a run in nonce order, in one burst,
//!   to one node is the way to maximize the chance they will be in one block and that they will execute in order.
//! * Batching delays gossiping a client's EVM transaction by up to [`BATCH_WINDOW`].
//! * A node fetches at most `max_in_flight_batch_fetches` batches at a time (see [`Config`]); a
//!   batch announced while that many are being fetched is ignored, so a peer cannot make the node
//!   hold an unbounded number of partially fetched batches in memory.
//! * A batch whose transactions cannot all be fetched within its fetch deadline (see
//!   [`fetch_deadline`]) is dropped, as is one whose transactions do not form a run. Nothing from
//!   it is stored, and it is not gossiped onwards.
//! * Batching is a gossip optimisation only. Nothing in block validation or execution depends on
//!   it, and a node that gossips EVM transactions individually is still correct.

mod config;

use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
    sync::Arc,
    time::Duration,
};

use datasize::DataSize;
use serde::Serialize;
use tracing::{debug, info, warn};

use casper_types::{
    evm, Chainspec, InvalidTransaction, TimeDiff, Timestamp, Transaction, TransactionId,
};

use crate::{
    components::{
        fetcher::{EmptyValidationMetadata, FetchResult, FetchedData},
        Component,
    },
    effect::{
        announcements::EvmTransactionBatcherAnnouncement,
        requests::{AcceptTransactionRequest, FetcherRequest},
        EffectBuilder, EffectExt, Effects,
    },
    types::{
        check_evm_transaction_batch_follows, evm_transaction_batch_order_key, BatchedTransaction,
        EvmTransactionBatch, EvmTransactionBatchId, MetaTransaction, NodeId,
    },
    utils::Source,
    NodeRng,
};

pub(crate) use config::Config;

const COMPONENT_NAME: &str = "evm_transaction_batcher";

/// How long a client's EVM transaction waits for further transactions from the same sender
/// before being gossiped.
pub(crate) const BATCH_WINDOW: Duration = Duration::from_millis(500);

#[derive(Debug, Serialize)]
pub(crate) enum Event {
    /// An EVM transaction from a client was accepted and is waiting to be gossiped.
    ClientTransactionAccepted(#[serde(skip_serializing)] Arc<Transaction>),
    /// The collection window opened for `sender` has elapsed.
    WindowElapsed {
        sender: evm::Address,
        window_id: u64,
    },
    /// A peer announced a batch, which it holds all of the transactions of.
    BatchAnnounced {
        batch_id: EvmTransactionBatchId,
        sender: NodeId,
    },
    /// The fetch of the next transaction of a batch being acquired has finished.
    TransactionFetched {
        batch_id: EvmTransactionBatchId,
        acquisition_id: u64,
        #[serde(skip_serializing)]
        result: FetchResult<BatchedTransaction>,
    },
    /// The deadline for fetching a batch's transactions has passed.
    AcquisitionDeadlineElapsed {
        batch_id: EvmTransactionBatchId,
        acquisition_id: u64,
    },
    /// The transactions of a batch received from `sender` have been through the acceptor.
    BatchAcceptanceFinished {
        batch_id: EvmTransactionBatchId,
        sender: NodeId,
        accepted: Vec<TransactionId>,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::ClientTransactionAccepted(transaction) => {
                write!(
                    formatter,
                    "client EVM transaction accepted: {}",
                    transaction.hash()
                )
            }
            Event::WindowElapsed { sender, window_id } => {
                write!(
                    formatter,
                    "batch window {} elapsed for {}",
                    window_id, sender
                )
            }
            Event::BatchAnnounced { batch_id, sender } => {
                write!(formatter, "{} announced {}", sender, batch_id)
            }
            Event::TransactionFetched {
                batch_id, result, ..
            } => match result {
                Ok(fetched) => write!(formatter, "{} for {}", fetched, batch_id),
                Err(error) => write!(formatter, "failed to fetch for {}: {}", batch_id, error),
            },
            Event::AcquisitionDeadlineElapsed { batch_id, .. } => {
                write!(formatter, "fetch deadline elapsed for {}", batch_id)
            }
            Event::BatchAcceptanceFinished {
                batch_id,
                sender,
                accepted,
            } => write!(
                formatter,
                "accepted {} of {} from {}",
                accepted.len(),
                batch_id,
                sender
            ),
        }
    }
}

/// A client's EVM transactions from one sender, waiting for their window to elapse.
#[derive(DataSize, Debug)]
struct PendingRun {
    window_id: u64,
    transactions: Vec<Arc<Transaction>>,
}

/// A batch announced by peers, whose transactions are being fetched one at a time.
#[derive(DataSize, Debug)]
struct Acquisition {
    /// Distinguishes this acquisition from an earlier, abandoned one of the same batch, so that
    /// late events for the earlier one are ignored.
    acquisition_id: u64,
    /// The peers which announced the batch, in the order they did.
    holders: Vec<NodeId>,
    /// The index into `holders` of the peer the next transaction is fetched from.
    holder_index: usize,
    /// How many holders have failed to provide the transaction currently being fetched.
    failed_attempts: usize,
    /// The batch's transactions fetched so far, in nonce order.
    fetched: Vec<Transaction>,
}

impl Acquisition {
    fn holder(&self) -> NodeId {
        self.holders[self.holder_index % self.holders.len()]
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct EvmTransactionBatcher {
    /// Fetched transactions are checked against it before they are held.
    chainspec: Arc<Chainspec>,
    pending: HashMap<evm::Address, PendingRun>,
    next_window_id: u64,
    /// Batches announced by peers whose transactions are being fetched.
    acquiring: HashMap<EvmTransactionBatchId, Acquisition>,
    next_acquisition_id: u64,
    /// The maximum number of transactions in a batch, built or fetched.
    max_batch_size: usize,
    /// The maximum number of batches in `acquiring`, or `None` if unlimited.
    max_in_flight_batch_fetches: Option<usize>,
    /// How long fetching a batch's transactions may take before the batch is dropped.
    fetch_deadline: Duration,
    /// The number of announced batches all of whose transactions were fetched.
    #[cfg(test)]
    fetched_batches: u64,
}

impl EvmTransactionBatcher {
    /// Creates a batcher which drops a batch whose transactions are not all fetched within
    /// `fetch_deadline`; see [`fetch_deadline`].
    pub(crate) fn new(config: Config, chainspec: Arc<Chainspec>, fetch_deadline: Duration) -> Self {
        EvmTransactionBatcher {
            chainspec,
            pending: HashMap::new(),
            next_window_id: 0,
            acquiring: HashMap::new(),
            next_acquisition_id: 0,
            max_batch_size: config.max_batch_size(),
            max_in_flight_batch_fetches: config.max_in_flight_batch_fetches(),
            fetch_deadline,
            #[cfg(test)]
            fetched_batches: 0,
        }
    }

    /// The number of announced batches all of whose transactions were fetched.
    #[cfg(test)]
    pub(crate) fn fetched_batches(&self) -> u64 {
        self.fetched_batches
    }

    fn handle_client_transaction<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        transaction: Arc<Transaction>,
    ) -> Effects<Event>
    where
        REv: From<EvmTransactionBatcherAnnouncement> + Send,
    {
        let Some(sender) = transaction
            .as_evm()
            .map(|evm_transaction| evm_transaction.from())
        else {
            return gossip_transaction(effect_builder, &transaction, Source::Client);
        };

        let mut effects = Effects::new();
        let run = self.pending.entry(sender).or_insert_with(|| {
            let window_id = self.next_window_id;
            self.next_window_id = self.next_window_id.wrapping_add(1);
            effects.extend(
                effect_builder
                    .set_timeout(BATCH_WINDOW)
                    .event(move |_| Event::WindowElapsed { sender, window_id }),
            );
            PendingRun {
                window_id,
                transactions: Vec::new(),
            }
        });
        run.transactions.push(transaction);
        if run.transactions.len() >= self.max_batch_size {
            effects.extend(self.flush(effect_builder, sender));
        }
        effects
    }

    fn handle_window_elapsed<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        sender: evm::Address,
        window_id: u64,
    ) -> Effects<Event>
    where
        REv: From<EvmTransactionBatcherAnnouncement> + Send,
    {
        // The run may have been flushed early (and a new window opened) since this timer was set.
        match self.pending.get(&sender) {
            Some(run) if run.window_id == window_id => self.flush(effect_builder, sender),
            _ => Effects::new(),
        }
    }

    /// Gossips a sender's waiting transactions as one batch in batch order, or a lone transaction
    /// on its own.
    fn flush<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        sender: evm::Address,
    ) -> Effects<Event>
    where
        REv: From<EvmTransactionBatcherAnnouncement> + Send,
    {
        let Some(run) = self.pending.remove(&sender) else {
            return Effects::new();
        };
        let transactions = in_batch_order(run.transactions);
        if let [transaction] = transactions.as_slice() {
            return gossip_transaction(effect_builder, transaction, Source::Client);
        }
        let batch_transactions = transactions
            .iter()
            .map(|transaction| (**transaction).clone())
            .collect();
        match EvmTransactionBatch::new(batch_transactions) {
            Ok(batch) => {
                debug!(%batch, %sender, "gossiping EVM transaction batch");
                effect_builder
                    .announce_evm_gossip(EvmTransactionBatcherAnnouncement::GossipBatch {
                        batch_id: batch.id(),
                        source: Source::Client,
                    })
                    .ignore()
            }
            Err(error) => {
                // `in_batch_order` only yields valid batches, but if one is rejected, gossiping
                // its transactions individually is still correct.
                warn!(%error, %sender, "could not batch EVM transactions; gossiping individually");
                transactions
                    .iter()
                    .flat_map(|transaction| {
                        gossip_transaction(effect_builder, transaction, Source::Client)
                    })
                    .collect()
            }
        }
    }

    fn handle_batch_announced<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        batch_id: EvmTransactionBatchId,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<BatchedTransaction>> + Send,
    {
        if let Some(acquisition) = self.acquiring.get_mut(&batch_id) {
            // Another peer holds the batch: fetch from it too if the first one fails.
            if !acquisition.holders.contains(&sender) {
                acquisition.holders.push(sender);
            }
            return Effects::new();
        }
        if batch_id.transaction_ids().len() > self.max_batch_size {
            debug!(
                %batch_id, %sender, max_batch_size = self.max_batch_size,
                "EVM transaction batch is larger than allowed; ignoring announced batch"
            );
            return Effects::new();
        }
        if self
            .max_in_flight_batch_fetches
            .is_some_and(|max| self.acquiring.len() >= max)
        {
            debug!(
                %batch_id, %sender,
                "too many EVM transaction batches being fetched; ignoring announced batch"
            );
            return Effects::new();
        }
        let acquisition_id = self.next_acquisition_id;
        self.next_acquisition_id = self.next_acquisition_id.wrapping_add(1);
        let acquisition = Acquisition {
            acquisition_id,
            holders: vec![sender],
            holder_index: 0,
            failed_attempts: 0,
            fetched: Vec::with_capacity(batch_id.transaction_ids().len()),
        };
        let mut effects = fetch_next(effect_builder, &batch_id, &acquisition);
        let deadline_batch_id = batch_id.clone();
        effects.extend(
            effect_builder
                .set_timeout(self.fetch_deadline)
                .event(move |_| Event::AcquisitionDeadlineElapsed {
                    batch_id: deadline_batch_id,
                    acquisition_id,
                }),
        );
        self.acquiring.insert(batch_id, acquisition);
        effects
    }

    fn handle_transaction_fetched<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        batch_id: EvmTransactionBatchId,
        acquisition_id: u64,
        result: FetchResult<BatchedTransaction>,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<BatchedTransaction>> + From<AcceptTransactionRequest> + Send,
    {
        let Some(acquisition) = self
            .acquiring
            .get_mut(&batch_id)
            .filter(|acquisition| acquisition.acquisition_id == acquisition_id)
        else {
            // The batch was dropped, or completed, while this fetch was in flight.
            return Effects::new();
        };

        let transaction = match result {
            Ok(FetchedData::FromStorage { item } | FetchedData::FromPeer { item, .. }) => {
                item.into_inner()
            }
            Err(error) => {
                acquisition.failed_attempts += 1;
                if acquisition.failed_attempts >= acquisition.holders.len() {
                    debug!(
                        %batch_id, %error,
                        "no peer provided a transaction of an EVM transaction batch; dropping it"
                    );
                    self.acquiring.remove(&batch_id);
                    return Effects::new();
                }
                acquisition.holder_index += 1;
                return fetch_next(effect_builder, &batch_id, acquisition);
            }
        };

        // An invalid transaction drops the whole batch, including the transactions already
        // fetched. The gossiper stops waiting for it after its validate-and-store timeout.
        if let Err(error) =
            check_evm_transaction_batch_follows(acquisition.fetched.last(), &transaction)
        {
            warn!(%batch_id, %error, "received invalid EVM transaction batch");
            self.acquiring.remove(&batch_id);
            return Effects::new();
        }
        if let Err(error) = check_config_compliant(&transaction, &self.chainspec) {
            warn!(%batch_id, %error, "received EVM transaction batch with an invalid transaction");
            self.acquiring.remove(&batch_id);
            return Effects::new();
        }
        acquisition.fetched.push(transaction);
        acquisition.failed_attempts = 0;
        if acquisition.fetched.len() < batch_id.transaction_ids().len() {
            return fetch_next(effect_builder, &batch_id, acquisition);
        }

        let Some(acquisition) = self.acquiring.remove(&batch_id) else {
            return Effects::new();
        };
        #[cfg(test)]
        {
            self.fetched_batches += 1;
        }
        let sender = acquisition.holders[0];
        match EvmTransactionBatch::new(acquisition.fetched) {
            Ok(batch) => self.accept_batch(effect_builder, batch, sender),
            Err(error) => {
                warn!(%batch_id, %error, "received invalid EVM transaction batch");
                Effects::new()
            }
        }
    }

    fn handle_acquisition_deadline_elapsed(
        &mut self,
        batch_id: EvmTransactionBatchId,
        acquisition_id: u64,
    ) -> Effects<Event> {
        let is_current = self
            .acquiring
            .get(&batch_id)
            .is_some_and(|acquisition| acquisition.acquisition_id == acquisition_id);
        if is_current {
            debug!(
                %batch_id,
                "could not fetch all transactions of an EVM transaction batch in time; dropping it"
            );
            self.acquiring.remove(&batch_id);
        }
        Effects::new()
    }

    /// Passes the transactions of a fully fetched batch to the transaction acceptor, which stores
    /// those it accepts.
    fn accept_batch<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        batch: EvmTransactionBatch,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: From<AcceptTransactionRequest> + Send,
    {
        let batch_id = batch.id();
        let transactions = batch.into_transactions();
        async move {
            // Accepting in nonce order keeps the stored and announced order of the run
            // consistent with the order in which it will be proposed.
            let mut accepted = Vec::with_capacity(transactions.len());
            for transaction in transactions {
                let transaction_id = transaction.compute_id();
                match effect_builder
                    .try_accept_batched_transaction(transaction, sender)
                    .await
                {
                    Ok(()) => accepted.push(transaction_id),
                    Err(error) => {
                        debug!(%transaction_id, %sender, %error, "rejected EVM transaction from batch")
                    }
                }
            }
            accepted
        }
        .event(move |accepted| Event::BatchAcceptanceFinished {
            batch_id,
            sender,
            accepted,
        })
    }

    fn handle_batch_acceptance_finished<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        batch_id: EvmTransactionBatchId,
        sender: NodeId,
        accepted: Vec<TransactionId>,
    ) -> Effects<Event>
    where
        REv: From<EvmTransactionBatcherAnnouncement> + Send,
    {
        let source = Source::PeerGossiped(sender);
        if accepted.len() == batch_id.transaction_ids().len() {
            return effect_builder
                .announce_evm_gossip(EvmTransactionBatcherAnnouncement::GossipBatch {
                    batch_id,
                    source,
                })
                .ignore();
        }
        // The batch as a whole cannot be passed on, but the transactions which were accepted
        // should still reach other nodes and this node's transaction buffer.
        info!(
            %batch_id,
            %sender,
            accepted = accepted.len(),
            "not all transactions of an EVM transaction batch were accepted; gossiping the \
            accepted ones individually"
        );
        accepted
            .into_iter()
            .flat_map(|transaction_id| {
                effect_builder
                    .announce_evm_gossip(EvmTransactionBatcherAnnouncement::GossipTransaction {
                        transaction_id,
                        source: source.clone(),
                    })
                    .ignore()
            })
            .collect()
    }
}

/// How long fetching a batch's transactions may take: one fetcher peer timeout for each
/// transaction of the largest batch, `max_batch_size`.
///
/// The batch gossiper's `validate_and_store_timeout` must be extended by this much. If the
/// gossiper gave up on a batch while its transactions were still being fetched, it would hand them
/// to the transaction buffer before they were stored, and once accepted they would never become
/// proposable on this node.
pub(crate) fn fetch_deadline(get_from_peer_timeout: TimeDiff, max_batch_size: usize) -> TimeDiff {
    get_from_peer_timeout.saturating_mul(max_batch_size as u64)
}

/// Checks a fetched transaction against the chainspec, as the transaction acceptor will once the
/// whole batch has arrived: it must fit an EVM lane, have the right chain ID and acceptable fees.
///
/// Unlike the acceptor's other checks, these don't read global state, so they can run before the
/// transaction is held. In particular, a transaction larger than any EVM lane allows is rejected
/// here instead of being held in memory until the batch is complete.
fn check_config_compliant(
    transaction: &Transaction,
    chainspec: &Chainspec,
) -> Result<(), InvalidTransaction> {
    MetaTransaction::from_transaction(
        transaction,
        chainspec.core_config.pricing_handling,
        &chainspec.transaction_config,
        &chainspec.evm_config,
    )?
    // The timestamp arguments only apply to non-EVM transactions, which never get this far.
    .is_config_compliant(chainspec, TimeDiff::default(), Timestamp::now())
}

/// Fetches the next transaction of a batch being acquired.
fn fetch_next<REv>(
    effect_builder: EffectBuilder<REv>,
    batch_id: &EvmTransactionBatchId,
    acquisition: &Acquisition,
) -> Effects<Event>
where
    REv: From<FetcherRequest<BatchedTransaction>> + Send,
{
    let Some(transaction_id) = batch_id
        .transaction_ids()
        .get(acquisition.fetched.len())
        .copied()
    else {
        return Effects::new();
    };
    let batch_id = batch_id.clone();
    let acquisition_id = acquisition.acquisition_id;
    effect_builder
        .fetch::<BatchedTransaction>(
            transaction_id,
            acquisition.holder(),
            Box::new(EmptyValidationMetadata),
        )
        .event(move |result| Event::TransactionFetched {
            batch_id,
            acquisition_id,
            result,
        })
}

fn gossip_transaction<REv>(
    effect_builder: EffectBuilder<REv>,
    transaction: &Transaction,
    source: Source,
) -> Effects<Event>
where
    REv: From<EvmTransactionBatcherAnnouncement> + Send,
{
    effect_builder
        .announce_evm_gossip(EvmTransactionBatcherAnnouncement::GossipTransaction {
            transaction_id: transaction.compute_id(),
            source,
        })
        .ignore()
}

/// Sorts a sender's transactions into batch order, by nonce and then by hash, dropping repeats of
/// the same transaction. Transactions sharing a nonce all stay in.
///
/// No more than `max_batch_size` transactions are ever waiting, as reaching it flushes them.
fn in_batch_order(mut transactions: Vec<Arc<Transaction>>) -> Vec<Arc<Transaction>> {
    transactions.sort_by_cached_key(|transaction| evm_transaction_batch_order_key(transaction));
    transactions.dedup_by_key(|transaction| transaction.hash());
    transactions
}

impl<REv> Component<REv> for EvmTransactionBatcher
where
    REv: From<EvmTransactionBatcherAnnouncement>
        + From<AcceptTransactionRequest>
        + From<FetcherRequest<BatchedTransaction>>
        + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::ClientTransactionAccepted(transaction) => {
                self.handle_client_transaction(effect_builder, transaction)
            }
            Event::WindowElapsed { sender, window_id } => {
                self.handle_window_elapsed(effect_builder, sender, window_id)
            }
            Event::BatchAnnounced { batch_id, sender } => {
                self.handle_batch_announced(effect_builder, batch_id, sender)
            }
            Event::TransactionFetched {
                batch_id,
                acquisition_id,
                result,
            } => self.handle_transaction_fetched(effect_builder, batch_id, acquisition_id, result),
            Event::AcquisitionDeadlineElapsed {
                batch_id,
                acquisition_id,
            } => self.handle_acquisition_deadline_elapsed(batch_id, acquisition_id),
            Event::BatchAcceptanceFinished {
                batch_id,
                sender,
                accepted,
            } => self.handle_batch_acceptance_finished(effect_builder, batch_id, sender, accepted),
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

#[cfg(test)]
mod tests;
