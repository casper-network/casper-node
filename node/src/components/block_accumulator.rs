mod block_acceptor;
mod config;
mod error;
mod event;

use std::{
    collections::{btree_map::Entry, BTreeMap},
    iter,
};

use datasize::DataSize;
use futures::FutureExt;
use tracing::{debug, error, warn};

use casper_types::{EraId, TimeDiff, Timestamp};

use crate::{
    components::Component,
    effect::{announcements::PeerBehaviorAnnouncement, EffectBuilder, EffectExt, Effects},
    types::{Block, BlockHash, BlockSignatures, FinalitySignature, NodeId, ValidatorMatrix},
    NodeRng,
};

use crate::effect::{
    announcements::BlockAccumulatorAnnouncement,
    requests::{BlockAccumulatorRequest, StorageRequest},
};
use block_acceptor::{BlockAcceptor, ShouldStore};
pub(crate) use config::Config;
use error::Error;
pub(crate) use event::Event;

#[derive(Debug)]
pub(crate) enum SyncInstruction {
    Leap,
    CaughtUp,
    BlockExec {
        next_block_hash: Option<BlockHash>,
    },
    BlockSync {
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum StartingWith {
    ExecutableBlock(BlockHash, u64),
    BlockIdentifier(BlockHash, u64),
    SyncedBlockIdentifier(BlockHash, u64),
    Hash(BlockHash),
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            StartingWith::BlockIdentifier(hash, _) => *hash,
            StartingWith::SyncedBlockIdentifier(hash, _) => *hash,
            StartingWith::ExecutableBlock(hash, _) => *hash,
            StartingWith::Hash(hash) => *hash,
        }
    }

    pub(crate) fn have_block(&self) -> bool {
        match self {
            StartingWith::BlockIdentifier(..) => true,
            StartingWith::ExecutableBlock(..) => true,
            StartingWith::SyncedBlockIdentifier(..) => true,
            StartingWith::Hash(_) => false,
        }
    }
}

/// A cache of pending blocks and finality signatures that are gossiped to this node.
///
/// Announces new blocks and finality signatures once they become valid.
#[derive(DataSize, Debug)]
pub(crate) struct BlockAccumulator {
    validator_matrix: ValidatorMatrix,
    attempt_execution_threshold: u64,
    dead_air_interval: TimeDiff,

    block_acceptors: BTreeMap<BlockHash, BlockAcceptor>,
    block_children: BTreeMap<BlockHash, BlockHash>,

    last_progress: Timestamp,
    /// The height of the subjective local tip of the chain.
    local_tip: Option<u64>,
}

fn store_block_and_finality_signatures<REv>(
    effect_builder: EffectBuilder<REv>,
    should_store: ShouldStore,
) -> Effects<Event>
where
    REv: From<StorageRequest> + Send,
{
    match should_store {
        ShouldStore::SufficientlySignedBlock { block, signatures } => {
            let mut block_signatures = BlockSignatures::new(*block.hash(), block.header().era_id());
            signatures.iter().for_each(|signature| {
                block_signatures.insert_proof(signature.public_key.clone(), signature.signature);
            });
            effect_builder
                .put_block_to_storage(Box::new(block.clone()))
                .then(move |_| effect_builder.put_signatures_to_storage(block_signatures))
                .event(move |_| Event::Stored {
                    block: Some(Box::new(block)),
                    finality_signatures: signatures,
                })
        }
        ShouldStore::SingleSignature(signature) => effect_builder
            .put_finality_signature_to_storage(signature.clone())
            .event(move |_| Event::Stored {
                block: None,
                finality_signatures: vec![signature],
            }),
        ShouldStore::Nothing => Effects::new(),
    }
}

impl BlockAccumulator {
    pub(crate) fn new(
        config: Config,
        validator_matrix: ValidatorMatrix,
        local_tip: Option<u64>,
    ) -> Self {
        Self {
            validator_matrix,
            attempt_execution_threshold: config.attempt_execution_threshold(),
            dead_air_interval: config.dead_air_interval(),
            block_acceptors: Default::default(),
            block_children: Default::default(),
            last_progress: Timestamp::now(),
            local_tip,
        }
    }

    pub(crate) fn sync_instruction(&mut self, starting_with: StartingWith) -> SyncInstruction {
        // BEFORE the f-seq cant help you, LEAP
        // ? |------------- future chain ------------------------>
        // IN f-seq not in range of tip, LEAP
        // |------------- future chain ----?-ATTEMPT_EXECUTION_THRESHOLD->
        // IN f-seq in range of tip, CAUGHT UP (which will ultimately result in EXEC)
        // |------------- future chain ----?ATTEMPT_EXECUTION_THRESHOLD>
        // AFTER the f-seq cant help you, SYNC-all-state
        // |------------- future chain ------------------------> ?
        let should_fetch_execution_state = false == starting_with.have_block();

        let maybe_highest_usable_block_height = self.highest_usable_block_height();

        match starting_with {
            StartingWith::ExecutableBlock(block_hash, block_height) => {
                // keep up only
                match maybe_highest_usable_block_height {
                    None => {
                        return SyncInstruction::BlockExec {
                            next_block_hash: None,
                        };
                    }
                    Some(highest_perceived) => {
                        if block_height > highest_perceived {
                            self.block_acceptors
                                .insert(block_hash, BlockAcceptor::new(block_hash, vec![]));
                            return SyncInstruction::BlockExec {
                                next_block_hash: None,
                            };
                        }
                        if highest_perceived == block_height {
                            return SyncInstruction::BlockExec {
                                next_block_hash: None,
                            };
                        }
                        if highest_perceived.saturating_sub(self.attempt_execution_threshold)
                            <= block_height
                        {
                            return SyncInstruction::BlockExec {
                                next_block_hash: self.next_syncable_block_hash(block_hash),
                            };
                        }
                    }
                }
            }
            StartingWith::Hash(block_hash) => {
                let (block_hash, maybe_block_height) = match self.block_acceptors.get(&block_hash) {
                    None => {
                        // the accumulator is unaware of the starting-with block
                        return SyncInstruction::Leap;
                    }
                    Some(block_acceptor) => {
                        (block_acceptor.block_hash(), block_acceptor.block_height())
                    }
                };
                if self.should_sync(maybe_block_height, maybe_highest_usable_block_height) {
                    self.last_progress = Timestamp::now();
                    return SyncInstruction::BlockSync {
                        block_hash,
                        should_fetch_execution_state,
                    };
                }
            }
            StartingWith::BlockIdentifier(block_hash, block_height) => {
                // catch up only
                if self.should_sync(Some(block_height), maybe_highest_usable_block_height) {
                    self.last_progress = Timestamp::now();
                    return SyncInstruction::BlockSync {
                        block_hash,
                        should_fetch_execution_state,
                    };
                }
            }
            StartingWith::SyncedBlockIdentifier(block_hash, block_height) => {
                // catch up only
                if self.should_sync(Some(block_height), maybe_highest_usable_block_height) {
                    if let Some(child_hash) = self.next_syncable_block_hash(block_hash) {
                        self.last_progress = Timestamp::now();
                        return SyncInstruction::BlockSync {
                            block_hash: child_hash,
                            should_fetch_execution_state,
                        };
                    } else if self.last_progress.elapsed() < self.dead_air_interval {
                        return SyncInstruction::CaughtUp;
                    }
                }
            }
        }
        SyncInstruction::Leap
    }

    fn should_sync(
        &mut self,
        maybe_starting_with_block_height: Option<u64>,
        maybe_highest_usable_block_height: Option<u64>,
    ) -> bool {
        match (
            maybe_starting_with_block_height,
            maybe_highest_usable_block_height,
        ) {
            (None, _) | (_, None) => false,
            (Some(starting_with), Some(highest_usable_block_height)) => {
                let height_diff = highest_usable_block_height.saturating_sub(starting_with);
                if height_diff == 0 {
                    true
                } else {
                    height_diff <= self.attempt_execution_threshold
                }
            }
        }
    }

    fn next_syncable_block_hash(&mut self, parent_block_hash: BlockHash) -> Option<BlockHash> {
        let child_hash = self.block_children.get(&parent_block_hash)?;
        let block_acceptor = self.block_acceptors.get_mut(child_hash)?;
        block_acceptor
            .has_sufficient_finality()
            .then(|| *child_hash)
    }

    fn register_block<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block: Block,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: From<StorageRequest> + From<PeerBehaviorAnnouncement> + Send,
    {
        let block_hash = block.hash();
        let era_id = block.header().era_id();
        let block_height = block.header().height();
        if self
            .local_tip
            .map_or(false, |curr_height| block_height < curr_height)
        {
            debug!(%block_hash, "ignoring outdated block");
            return Effects::new();
        }
        if let Some(parent_hash) = block.parent() {
            self.block_children.insert(*parent_hash, *block_hash);
        }

        let acceptor = match self.get_or_register_acceptor_mut(*block_hash, era_id, vec![sender]) {
            Some(block_gossip_acceptor) => block_gossip_acceptor,
            None => {
                return Effects::new();
            }
        };

        match acceptor.register_block(block, sender) {
            Ok(should_store) => store_block_and_finality_signatures(effect_builder, should_store),
            Err(Error::InvalidGossip(error)) => {
                warn!(%error, "received invalid finality_signature");
                effect_builder
                    .announce_disconnect_from_peer(error.peer())
                    .ignore()
            }
            Err(Error::EraMismatch(error)) => {
                // the acceptor logic purges finality signatures that don't match
                // the era validators, so in this case we can continue to
                // use the acceptor
                warn!(%error, "finality signature has mismatched era_id");

                // TODO: Log?
                // this block acceptor is borked; get rid of it?
                Effects::new()
            }
            Err(ref error @ Error::BlockHashMismatch { peer, .. }) => {
                warn!(%error, "finality signature has mismatched block_hash");
                effect_builder.announce_disconnect_from_peer(peer).ignore()
            }
            Err(Error::RemovedValidatorWeights { era_id }) => {
                if self.validator_matrix.validator_weights(era_id).is_some() {
                    return self.register_updated_validator_matrix(effect_builder);
                }
                Effects::new()
            }
            Err(Error::InvalidState) => Effects::new(),
        }
    }

    fn purge(&mut self) {
        // todo!: discuss w/ team if this approach or sth similar is acceptable
        let now = Timestamp::now();
        const PURGE_INTERVAL: u64 = 21600; // 6 hours
        let mut purged = vec![];
        self.block_acceptors.retain(|k, v| {
            let expired = now.saturating_diff(v.last_progress()) > TimeDiff::from(PURGE_INTERVAL);
            if expired {
                purged.push(*k)
            }
            !expired
        });
        self.block_children
            .retain(|_parent, child| false == purged.contains(child));
    }

    fn register_finality_signature<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finality_signature: FinalitySignature,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: From<StorageRequest> + From<PeerBehaviorAnnouncement> + Send,
    {
        // TODO: Also ignore signatures for blocks older than the highest complete one?
        // TODO: Ignore signatures for `already_handled` blocks?

        let block_hash = finality_signature.block_hash;
        let era_id = finality_signature.era_id;

        let acceptor = match self.get_or_register_acceptor_mut(block_hash, era_id, vec![sender]) {
            Some(block_gossip_acceptor) => block_gossip_acceptor,
            None => {
                return Effects::new();
            }
        };

        match acceptor.register_finality_signature(finality_signature, sender) {
            Ok(should_store) => store_block_and_finality_signatures(effect_builder, should_store),
            Err(Error::InvalidGossip(error)) => {
                warn!(%error, "received invalid finality_signature");
                effect_builder
                    .announce_disconnect_from_peer(error.peer())
                    .ignore()
            }
            Err(Error::EraMismatch(error)) => {
                // the acceptor logic purges finality signatures that don't match
                // the era validators, so in this case we can continue to
                // use the acceptor
                warn!(%error, "finality signature has mismatched era_id");
                Effects::new()
            }
            Err(ref error @ Error::BlockHashMismatch { peer, .. }) => {
                warn!(%error, "finality signature has mismatched block_hash");
                effect_builder.announce_disconnect_from_peer(peer).ignore()
            }
            Err(Error::RemovedValidatorWeights { era_id }) => {
                error!(%era_id, "should not remove validator weights when registering finality signatures");
                Effects::new()
            }
            Err(Error::InvalidState) => Effects::new(),
        }
    }

    pub(crate) fn register_updated_validator_matrix<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<StorageRequest> + Send,
    {
        let mut effects = Effects::new();

        for block_acceptor in self
            .block_acceptors
            .values_mut()
            .filter(|acceptor| !acceptor.has_validator_weights())
        {
            if let Some(era_id) = block_acceptor.era_id() {
                if let Some(validator_weights) = self.validator_matrix.validator_weights(era_id) {
                    match block_acceptor.refresh(validator_weights.clone()) {
                        Ok(new_effects) => effects.extend(store_block_and_finality_signatures(
                            effect_builder,
                            new_effects,
                        )),
                        Err(err) => {
                            error!(%err, "failed to register new validator weights");
                        }
                    }
                }
            }
        }
        effects
    }

    /// Drops all old block acceptors and tracks new local block height;
    /// subsequent attempts to register a block lower than tip will be rejected.
    pub(crate) fn register_local_tip(&mut self, height: u64) {
        self.purge();
        self.local_tip = self.local_tip.into_iter().chain(iter::once(height)).max();
    }

    fn highest_usable_block_height(&mut self) -> Option<u64> {
        let mut ret: Option<u64> = self.local_tip;
        for block_acceptor in &mut self.block_acceptors.values_mut() {
            if false == block_acceptor.has_sufficient_finality() {
                continue;
            }
            match block_acceptor.block_height() {
                None => {
                    continue;
                }
                Some(acceptor_height) => {
                    if let Some(curr_height) = ret {
                        if acceptor_height <= curr_height {
                            continue;
                        }
                    }
                    ret = Some(acceptor_height);
                }
            };
        }
        ret
    }

    fn get_or_register_acceptor_mut(
        &mut self,
        block_hash: BlockHash,
        era_id: EraId,
        peers: Vec<NodeId>,
    ) -> Option<&mut BlockAcceptor> {
        if let Entry::Occupied(mut entry) = self.block_acceptors.entry(block_hash) {
            if let Some(evw) = self.validator_matrix.validator_weights(era_id) {
                let acceptor = BlockAcceptor::new_with_validator_weights(block_hash, evw, peers);
                entry.insert(acceptor);
            } else {
                entry.insert(BlockAcceptor::new(block_hash, peers));
            }
        }

        self.block_acceptors.get_mut(&block_hash)
    }

    fn get_peers(&self, block_hash: BlockHash) -> Option<Vec<NodeId>> {
        self.block_acceptors
            .get(&block_hash)
            .map(BlockAcceptor::peers)
    }

    fn handle_stored<REv>(
        &self,
        effect_builder: EffectBuilder<REv>,
        block: Option<Box<Block>>,
        finality_signatures: Vec<FinalitySignature>,
    ) -> Effects<Event>
    where
        REv: From<BlockAccumulatorAnnouncement> + Send,
    {
        let mut effects = if let Some(block) = block {
            effect_builder.announce_block_accepted(block).ignore()
        } else {
            Effects::new()
        };
        for finality_signature in finality_signatures {
            effects.extend(
                effect_builder
                    .announce_finality_signature_accepted(Box::new(finality_signature))
                    .ignore(),
            );
        }
        effects
    }
}

impl<REv> Component<REv> for BlockAccumulator
where
    REv: From<StorageRequest>
        + From<PeerBehaviorAnnouncement>
        + From<BlockAccumulatorAnnouncement>
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
            Event::Request(BlockAccumulatorRequest::GetPeersForBlock {
                block_hash,
                responder,
            }) => responder.respond(self.get_peers(block_hash)).ignore(),
            Event::ReceivedBlock { block, sender } => {
                self.register_block(effect_builder, *block, sender)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => self.register_finality_signature(effect_builder, *finality_signature, sender),
            Event::ExecutedBlock { block_header } => {
                self.register_local_tip(block_header.height());
                Effects::new()
            }
            Event::Stored {
                block,
                finality_signatures,
            } => self.handle_stored(effect_builder, block, finality_signatures),
        }
    }
}
