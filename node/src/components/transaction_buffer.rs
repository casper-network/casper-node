mod config;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use std::{
    collections::{btree_map, BTreeMap, HashMap, HashSet, VecDeque},
    convert::TryInto,
    iter::FromIterator,
    mem,
    sync::Arc,
};

use datasize::DataSize;
use futures::FutureExt;
use itertools::Itertools;
use prometheus::Registry;
use smallvec::smallvec;
use tracing::{debug, error, info, warn};

use casper_types::{
    Block, BlockV2, Chainspec, Digest, DisplayIter, Timestamp, Transaction, TransactionCategory,
    TransactionHash, TransactionId,
};

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        Component, ComponentState, InitializedComponent,
    },
    effect::{
        announcements::TransactionBufferAnnouncement,
        requests::{StorageRequest, TransactionBufferRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    reactor::main_reactor::MainEvent,
    storage::Storage,
    types::{
        appendable_block::{AddError, AppendableBlock},
        FinalizedBlock, TransactionExt, TransactionFootprint,
    },
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;

use metrics::Metrics;

const COMPONENT_NAME: &str = "transaction_buffer";

#[derive(DataSize, Debug)]
pub(crate) struct TransactionBuffer {
    state: ComponentState,
    cfg: Config,
    chainspec: Arc<Chainspec>,
    // Keeps track of all transactions the buffer is currently aware of.
    //
    // `hold` and `dead` are used to filter it on demand as necessary.
    //
    // The timestamp is the time when the transaction expires.
    // Expired items are removed via a self-perpetuating expire event.
    buffer: HashMap<TransactionHash, (Timestamp, Option<TransactionFootprint>)>,
    // When a maybe-block is in flight, we pause inclusion of the transactions within it in other
    // proposed blocks. If the maybe-block becomes an actual block the transaction hashes will get
    // put to self.dead, otherwise, the hold will be released and the transactions will become
    // eligible to propose again.
    hold: BTreeMap<Timestamp, HashSet<TransactionHash>>,
    // Transaction hashes that should not be proposed, ever.
    dead: HashSet<TransactionHash>,
    #[data_size(skip)]
    metrics: Metrics,
}

impl TransactionBuffer {
    /// Create a transaction buffer.
    pub(crate) fn new(
        chainspec: Arc<Chainspec>,
        cfg: Config,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(TransactionBuffer {
            state: ComponentState::Uninitialized,
            cfg,
            chainspec,
            buffer: HashMap::new(),
            hold: BTreeMap::new(),
            dead: HashSet::new(),
            metrics: Metrics::new(registry)?,
        })
    }

    pub(crate) fn initialize_component(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        storage: &Storage,
    ) -> Option<Effects<MainEvent>> {
        if <Self as InitializedComponent<MainEvent>>::is_uninitialized(self) {
            info!(
                "pending initialization of {}",
                <Self as Component<MainEvent>>::name(self)
            );
            <Self as InitializedComponent<MainEvent>>::set_state(
                self,
                ComponentState::Initializing,
            );
            let blocks = match storage.read_blocks_for_replay_protection() {
                Ok(blocks) => blocks,
                Err(err) => {
                    return Some(
                        fatal!(
                            effect_builder,
                            "fatal block store error when attempting to read highest blocks: {}",
                            err
                        )
                        .ignore(),
                    )
                }
            };
            debug!(
                blocks = ?blocks.iter().map(|b| b.height()).collect_vec(),
                "TransactionBuffer: initialization"
            );
            info!("initialized {}", <Self as Component<MainEvent>>::name(self));
            let event = Event::Initialize(blocks);
            return Some(smallvec![async {
                smallvec![MainEvent::TransactionBuffer(event)]
            }
            .boxed()]);
        }
        if <Self as InitializedComponent<MainEvent>>::is_fatal(self) {
            return Some(
                fatal!(
                    effect_builder,
                    "{} failed to initialize",
                    <Self as Component<MainEvent>>::name(self)
                )
                .ignore(),
            );
        }
        None
    }

    /// Manages cache ejection.
    fn expire<REv>(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<Event> + From<TransactionBufferAnnouncement> + Send,
    {
        let now = Timestamp::now();
        let (buffer, mut freed): (HashMap<_, _>, _) = mem::take(&mut self.buffer)
            .into_iter()
            .partition(|(_, (expiry_time, _))| *expiry_time >= now);

        if !freed.is_empty() {
            info!("TransactionBuffer: purging {} transaction(s)", freed.len());
        }

        // clear expired transaction from all holds, then clear any entries that have no items
        // remaining
        self.hold.iter_mut().for_each(|(_, held_transactions)| {
            held_transactions.retain(|transaction_hash| !freed.contains_key(transaction_hash))
        });
        self.hold.retain(|_, remaining| !remaining.is_empty());

        // retain all those in `dead` which are not in `freed`, at the same time reducing `freed` to
        // only those entries not also in `dead` - i.e. transactions which expired without being
        // included in a block
        self.dead
            .retain(|transaction_hash| freed.remove(transaction_hash).is_none());
        self.buffer = buffer;

        if !freed.is_empty() {
            info!(
                "TransactionBuffer: expiring without executing {} transaction(s)",
                freed.len()
            );
            debug!(
                "TransactionBuffer: expiring without executing {}",
                DisplayIter::new(freed.keys())
            );
        }

        let mut effects = effect_builder
            .announce_expired_transactions(freed.keys().cloned().collect())
            .ignore();
        effects.extend(
            effect_builder
                .set_timeout(self.cfg.expiry_check_interval().into())
                .event(move |_| Event::Expire),
        );
        self.update_all_metrics();
        effects
    }

    fn register_transaction_gossiped<REv>(
        &mut self,
        transaction_id: TransactionId,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<Event> + From<StorageRequest> + Send,
    {
        debug!(%transaction_id, "TransactionBuffer: registering gossiped transaction");
        effect_builder
            .get_stored_transaction(transaction_id)
            .event(move |maybe_transaction| {
                Event::StoredTransaction(transaction_id, maybe_transaction.map(Box::new))
            })
    }

    /// Update buffer considering new stored transaction.
    fn register_transaction(&mut self, transaction: Transaction) {
        let transaction_hash = transaction.hash();
        if transaction.verify().is_err() {
            error!(%transaction_hash, "TransactionBuffer: invalid transaction must not be buffered");
            return;
        }
        if self.dead.contains(&transaction_hash) {
            info!(%transaction_hash, "TransactionBuffer: attempt to register already dead transaction");
            return;
        }
        if self
            .hold
            .values()
            .any(|ths| ths.contains(&transaction_hash))
        {
            info!(%transaction_hash, "TransactionBuffer: attempt to register already held transaction");
            return;
        }
        let footprint = match transaction.footprint(&self.chainspec) {
            Some(footprint) => footprint,
            None => {
                error!(%transaction_hash, "TransactionBuffer: unable to created transaction footprint");
                return;
            }
        };
        let expiry_time = transaction.expires();
        match self
            .buffer
            .insert(transaction_hash, (expiry_time, Some(footprint)))
        {
            Some(prev) => {
                warn!(%transaction_hash, ?prev, "TransactionBuffer: transaction upserted");
            }
            None => {
                debug!(%transaction_hash, "TransactionBuffer: new transaction buffered");
                self.metrics.total_transactions.inc();
            }
        }
    }

    /// Update holds considering new proposed block.
    fn register_block_proposed(&mut self, proposed_block: ProposedBlock<ClContext>) {
        let timestamp = &proposed_block.context().timestamp();
        if let Some(hold_set) = self.hold.get_mut(timestamp) {
            debug!(%timestamp, "TransactionBuffer: existing hold timestamp extended");
            hold_set.extend(
                proposed_block
                    .value()
                    .all_transactions()
                    .map(|(transaction_hash, _)| *transaction_hash),
            );
        } else {
            debug!(%timestamp, "TransactionBuffer: new hold timestamp inserted");
            self.hold.insert(
                *timestamp,
                HashSet::from_iter(
                    proposed_block
                        .value()
                        .all_transactions()
                        .map(|(transaction_hash, _)| *transaction_hash),
                ),
            );
        }
        self.metrics.held_transactions.set(
            self.hold
                .values()
                .map(|transactions| transactions.len())
                .sum::<usize>()
                .try_into()
                .unwrap_or(i64::MIN),
        );
    }

    fn register_transactions<'a>(
        &mut self,
        timestamp: Timestamp,
        transaction_hashes: impl Iterator<Item = &'a TransactionHash>,
    ) {
        let expiry_timestamp = timestamp.saturating_add(self.chainspec.transaction_config.max_ttl);

        for transaction_hash in transaction_hashes {
            if !self.buffer.contains_key(transaction_hash) {
                self.buffer
                    .insert(*transaction_hash, (expiry_timestamp, None));
            }
            self.dead.insert(*transaction_hash);
        }
        // Transactions held for proposed blocks which did not get finalized in time are eligible
        // again
        let (hold, _) = mem::take(&mut self.hold)
            .into_iter()
            .partition(|(ts, _)| *ts > timestamp);
        self.hold = hold;
        self.update_all_metrics();
    }

    /// Update buffer and holds considering new added block.
    fn register_block(&mut self, block: &BlockV2) {
        let block_height = block.height();
        let timestamp = block.timestamp();
        debug!(%timestamp, "TransactionBuffer: register_block({}) timestamp finalized", block_height);
        self.register_transactions(timestamp, block.all_transactions());
    }

    /// When initializing the buffer, register past blocks in order to provide replay protection.
    fn register_versioned_block(&mut self, block: &Block) {
        let block_height = block.height();
        let timestamp = block.timestamp();
        debug!(
            %timestamp,
            "TransactionBuffer: register_versioned_block({}) timestamp finalized",
            block_height
        );
        match block {
            Block::V1(v1_block) => {
                let transaction_hashes: Vec<TransactionHash> = v1_block
                    .deploy_and_transfer_hashes()
                    .map(|deploy_hash| TransactionHash::Deploy(*deploy_hash))
                    .collect();
                self.register_transactions(timestamp, transaction_hashes.iter())
            }
            Block::V2(v2_block) => {
                self.register_transactions(timestamp, v2_block.all_transactions());
            }
        }
    }

    /// Update buffer and holds considering new finalized block.
    fn register_block_finalized(&mut self, finalized_block: &FinalizedBlock) {
        let block_height = finalized_block.height;
        let timestamp = finalized_block.timestamp;
        debug!(%timestamp, "TransactionBuffer: register_block_finalized({}) timestamp finalized", block_height);
        self.register_transactions(timestamp, finalized_block.all_transactions());
    }

    /// Returns eligible transactions that are buffered and not held or dead.
    fn proposable(&self) -> Vec<(TransactionHash, TransactionFootprint)> {
        debug!("TransactionBuffer: getting proposable transactions");
        self.buffer
            .iter()
            .filter(|(th, _)| !self.hold.values().any(|hs| hs.contains(th)))
            .filter(|(th, _)| !self.dead.contains(th))
            .filter_map(|(th, (_, maybe_data))| {
                maybe_data
                    .as_ref()
                    .map(|footprint| (*th, footprint.clone()))
            })
            .collect()
    }

    fn buckets(&mut self) -> HashMap<Digest, Vec<(TransactionHash, TransactionFootprint)>> {
        let proposable = self.proposable();

        let mut buckets: HashMap<Digest, Vec<(TransactionHash, TransactionFootprint)>> =
            HashMap::new();

        for (transaction_hash, footprint) in proposable {
            let body_hash = footprint.body_hash;
            buckets
                .entry(body_hash)
                .and_modify(|vec| vec.push((transaction_hash, footprint.clone())))
                .or_insert(vec![(transaction_hash, footprint)]);
        }
        buckets
    }

    /// Returns a right-sized payload of transactions that can be proposed.
    fn appendable_block(&mut self, timestamp: Timestamp) -> AppendableBlock {
        let mut ret = AppendableBlock::new(self.chainspec.transaction_config, timestamp);
        let mut holds = HashSet::new();

        // TODO[RC]: It's error prone to use 4 different flags to track the limits. Implement a
        // proper limiter.
        let mut have_hit_mint_limit = false;
        let mut have_hit_standard_limit = false;
        let mut have_hit_install_upgrade_limit = false;
        let mut have_hit_auction_limit = false;

        let mut buckets = self.buckets();
        let mut body_hashes_queue: VecDeque<_> = buckets.keys().cloned().collect();

        #[cfg(test)]
        let mut iter_counter = 0;
        #[cfg(test)]
        let iter_limit = self.buffer.len() * 4;

        while let Some(body_hash) = body_hashes_queue.pop_front() {
            #[cfg(test)]
            {
                iter_counter += 1;
                assert!(
                    iter_counter < iter_limit,
                    "the number of iterations shouldn't be too large"
                );
            }

            let Some((transaction_hash,footprint)) = buckets.get_mut(&body_hash).and_then(Vec::<_>::pop)
            else {
                continue;
            };

            // bucket wasn't empty - push the hash back into the queue to be processed again on the
            // next pass
            body_hashes_queue.push_back(body_hash);

            if footprint.is_mint() && have_hit_mint_limit {
                continue;
            }
            if footprint.is_standard() && have_hit_standard_limit {
                continue;
            }
            if footprint.is_install_upgrade() && have_hit_install_upgrade_limit {
                continue;
            }
            if footprint.is_auction() && have_hit_auction_limit {
                continue;
            }

            // let transaction_hash = with_approvals.transaction_hash();
            let has_multiple_approvals = footprint.approvals.len() > 1;
            match ret.add_transaction(footprint.clone()) {
                Ok(_) => {
                    debug!(%transaction_hash, "TransactionBuffer: proposing transaction");
                    holds.insert(transaction_hash);
                }
                Err(error) => {
                    match error {
                        AddError::Duplicate => {
                            // it should be physically impossible for a duplicate transaction or
                            // transaction to be in the transaction buffer, thus this should be
                            // unreachable
                            error!(
                                ?transaction_hash,
                                "TransactionBuffer: duplicated transaction or transfer in transaction buffer"
                            );
                            self.dead.insert(transaction_hash);
                        }
                        AddError::Expired => {
                            info!(
                                ?transaction_hash,
                                "TransactionBuffer: expired transaction or transfer in transaction buffer"
                            );
                            self.dead.insert(transaction_hash);
                        }
                        AddError::Count(category) => {
                            match category {
                                TransactionCategory::Mint => {
                                    have_hit_mint_limit = true;
                                }
                                TransactionCategory::Auction => {
                                    have_hit_auction_limit = true;
                                }
                                TransactionCategory::Standard => {
                                    have_hit_standard_limit = true;
                                }
                                TransactionCategory::InstallUpgrade => {
                                    have_hit_install_upgrade_limit = true;
                                }
                            }
                            if have_hit_standard_limit
                                && have_hit_auction_limit
                                && have_hit_install_upgrade_limit
                                && have_hit_mint_limit
                            {
                                info!(
                                    ?transaction_hash,
                                    "TransactionBuffer: block fully saturated"
                                );
                                break;
                            }
                        }
                        AddError::ApprovalCount if has_multiple_approvals => {
                            // keep iterating, we can maybe fit in a deploy with fewer approvals
                        }
                        AddError::ApprovalCount | AddError::GasLimit | AddError::BlockSize => {
                            info!(
                                ?transaction_hash,
                                %error,
                                "TransactionBuffer: a block limit has been reached"
                            );
                            // a block limit has been reached
                            break;
                        }
                        AddError::VariantMismatch(mismatch) => {
                            error!(?transaction_hash, %mismatch,
                                "TransactionBuffer: data mismatch when adding transaction"
                            );
                            // keep iterating
                        }
                        AddError::ExcessiveTtl => {
                            error!(
                                ?transaction_hash,
                                "TransactionBuffer: skipping transaction with excessive ttl"
                            );
                            // keep iterating
                        }
                        AddError::FutureDatedDeploy => {
                            error!(
                                ?transaction_hash,
                                %footprint.timestamp,
                                "TransactionBuffer: skipping transaction with future dated deploy"
                            );
                            // keep iterating
                        }
                    }
                }
            }
        }

        // Put a hold on all proposed transactions / transfers and update metrics
        match self.hold.entry(timestamp) {
            btree_map::Entry::Vacant(entry) => {
                entry.insert(holds);
            }
            btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().extend(holds);
            }
        }
        self.update_all_metrics();

        info!(
            "produced {}, buffer has {} held, {} dead, {} total",
            ret,
            self.hold
                .values()
                .map(|transactions| transactions.len())
                .sum::<usize>(),
            self.dead.len(),
            self.buffer.len()
        );

        ret
    }

    /// Updates all transaction count metrics based on the size of the internal structs.
    fn update_all_metrics(&mut self) {
        // if number of elements is too high to fit, we overflow the metric
        // intentionally in order to get some indication that something is wrong.
        self.metrics.held_transactions.set(
            self.hold
                .values()
                .map(|transactions| transactions.len())
                .sum::<usize>()
                .try_into()
                .unwrap_or(i64::MIN),
        );
        self.metrics
            .dead_transactions
            .set(self.dead.len().try_into().unwrap_or(i64::MIN));
        self.metrics
            .total_transactions
            .set(self.buffer.len().try_into().unwrap_or(i64::MIN));
    }
}

impl<REv> InitializedComponent<REv> for TransactionBuffer
where
    REv: From<Event> + From<TransactionBufferAnnouncement> + From<StorageRequest> + Send + 'static,
{
    fn state(&self) -> &ComponentState {
        &self.state
    }

    fn set_state(&mut self, new_state: ComponentState) {
        info!(
            ?new_state,
            name = <Self as Component<MainEvent>>::name(self),
            "component state changed"
        );

        self.state = new_state;
    }
}

impl<REv> Component<REv> for TransactionBuffer
where
    REv: From<Event> + From<TransactionBufferAnnouncement> + From<StorageRequest> + Send + 'static,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.state {
            ComponentState::Fatal(msg) => {
                error!(
                    msg,
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            ComponentState::Uninitialized => {
                warn!(
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when component is uninitialized"
                );
                Effects::new()
            }
            ComponentState::Initializing => {
                match event {
                    Event::Initialize(blocks) => {
                        for block in blocks {
                            self.register_versioned_block(&block);
                        }
                        <Self as InitializedComponent<MainEvent>>::set_state(
                            self,
                            ComponentState::Initialized,
                        );
                        // start self-expiry management on initialization
                        effect_builder
                            .set_timeout(self.cfg.expiry_check_interval().into())
                            .event(move |_| Event::Expire)
                    }
                    Event::Request(_)
                    | Event::ReceiveTransactionGossiped(_)
                    | Event::StoredTransaction(_, _)
                    | Event::BlockProposed(_)
                    | Event::Block(_)
                    | Event::VersionedBlock(_)
                    | Event::BlockFinalized(_)
                    | Event::Expire => {
                        warn!(
                            ?event,
                            name = <Self as Component<MainEvent>>::name(self),
                            "should not handle this event when component is pending initialization"
                        );
                        Effects::new()
                    }
                }
            }
            ComponentState::Initialized => match event {
                Event::Initialize(_) => {
                    error!(
                        ?event,
                        name = <Self as Component<MainEvent>>::name(self),
                        "component already initialized"
                    );
                    Effects::new()
                }
                Event::Request(TransactionBufferRequest::GetAppendableBlock {
                    timestamp,
                    responder,
                }) => responder.respond(self.appendable_block(timestamp)).ignore(),
                Event::BlockFinalized(finalized_block) => {
                    self.register_block_finalized(&finalized_block);
                    Effects::new()
                }
                Event::Block(block) => {
                    self.register_block(&block);
                    Effects::new()
                }
                Event::VersionedBlock(block) => {
                    self.register_versioned_block(&block);
                    Effects::new()
                }
                Event::BlockProposed(proposed) => {
                    self.register_block_proposed(*proposed);
                    Effects::new()
                }
                Event::ReceiveTransactionGossiped(transaction_id) => {
                    self.register_transaction_gossiped(transaction_id, effect_builder)
                }
                Event::StoredTransaction(transaction_id, maybe_transaction) => {
                    match maybe_transaction {
                        Some(transaction) => {
                            self.register_transaction(*transaction);
                        }
                        None => {
                            warn!("cannot register un-stored transaction({})", transaction_id);
                        }
                    }
                    Effects::new()
                }
                Event::Expire => self.expire(effect_builder),
            },
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}
