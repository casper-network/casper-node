mod block_acceptor;
mod config;
mod error;
mod event;
mod starting_with;
mod sync_instruction;

use std::{collections::BTreeMap, iter};

use datasize::DataSize;
use futures::FutureExt;
use tracing::{debug, error, warn};

use casper_types::{TimeDiff, Timestamp};

use crate::{
    components::Component,
    effect::{
        announcements::{
            BlockAccumulatorAnnouncement, ControlAnnouncement, PeerBehaviorAnnouncement,
        },
        requests::{BlockAccumulatorRequest, BlockCompleteConfirmationRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    types::{Block, BlockHash, BlockSignatures, FinalitySignature, Item, NodeId, ValidatorMatrix},
    NodeRng,
};

use crate::components::ValidatorBoundComponent;
use block_acceptor::{BlockAcceptor, ShouldStore};
pub(crate) use config::Config;
use error::Error;
pub(crate) use event::Event;
pub(crate) use starting_with::StartingWith;
pub(crate) use sync_instruction::SyncInstruction;

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
        let should_fetch_execution_state = starting_with.should_fetch_execution_state();

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
        if block_acceptor.has_sufficient_finality() {
            Some(block_acceptor.block_hash())
        } else {
            None
        }
    }

    fn register_block<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block: Block,
        sender: Option<NodeId>,
    ) -> Effects<Event>
    where
        REv: From<StorageRequest>
            + From<PeerBehaviorAnnouncement>
            + From<ControlAnnouncement>
            + Send,
    {
        let block_hash = block.hash();

        error!(%block_hash, "XXXXX - register_block");

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

        let acceptor = self
            .block_acceptors
            .entry(*block_hash)
            .or_insert_with(|| BlockAcceptor::new(*block_hash, vec![]));

        error!(?acceptor, "XXXXX - acceptor");
        match acceptor.register_block(block, sender) {
            Ok(_) => match self.validator_matrix.validator_weights(era_id) {
                Some(evw) => store_block_and_finality_signatures(
                    effect_builder,
                    acceptor.should_store_block(&evw),
                ),
                None => Effects::new(),
            },
            Err(Error::InvalidGossip(error)) => {
                warn!(%error, "received invalid finality_signature");
                effect_builder
                    .announce_disconnect_from_peer(error.peer())
                    .ignore()
            }
            Err(Error::EraMismatch {
                peer,
                block_hash,
                expected,
                actual,
            }) => {
                warn!(
                    "era mismatch from {} for {}; expected: {} and actual: {}",
                    peer, block_hash, expected, actual
                );
                effect_builder.announce_disconnect_from_peer(peer).ignore()
            }
            Err(ref error @ Error::BlockHashMismatch { peer, .. }) => {
                warn!(%error, "finality signature has mismatched block_hash");
                effect_builder.announce_disconnect_from_peer(peer).ignore()
            }
            Err(ref error @ Error::SufficientFinalityWithoutBlock { .. }) => {
                error!(%error, "should not have sufficient finality without block");
                Effects::new()
            }
            Err(Error::InvalidConfiguration) => fatal!(
                effect_builder,
                "node has an invalid configuration, shutting down"
            )
            .ignore(),
        }
    }

    fn register_finality_signature<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finality_signature: FinalitySignature,
        sender: Option<NodeId>,
    ) -> Effects<Event>
    where
        REv: From<StorageRequest>
            + From<PeerBehaviorAnnouncement>
            + From<ControlAnnouncement>
            + Send,
    {
        // TODO: Also ignore signatures for blocks older than the highest complete one?
        // TODO: Ignore signatures for `already_handled` blocks?
        error!(id=%finality_signature.id(), "XXXXX - register_finality_signature");

        let block_hash = finality_signature.block_hash;
        let era_id = finality_signature.era_id;

        let acceptor = self.block_acceptors.entry(block_hash).or_insert_with(|| {
            BlockAcceptor::new(
                block_hash,
                sender.map(|sender| vec![sender]).unwrap_or_default(),
            )
        });

        match acceptor.register_finality_signature(finality_signature, sender) {
            Ok(Some(finality_signature)) => store_block_and_finality_signatures(
                effect_builder,
                ShouldStore::SingleSignature(finality_signature),
            ),
            Ok(None) => match &self.validator_matrix.validator_weights(era_id) {
                Some(evw) => store_block_and_finality_signatures(
                    effect_builder,
                    acceptor.should_store_block(evw),
                ),
                None => {
                    error!("XXXXX - received finality_signature, insufficent finality");
                    Effects::new()
                }
            },
            Err(Error::InvalidGossip(error)) => {
                warn!(%error, "received invalid finality_signature");
                effect_builder
                    .announce_disconnect_from_peer(error.peer())
                    .ignore()
            }
            Err(Error::EraMismatch {
                peer,
                block_hash,
                expected,
                actual,
            }) => {
                // the acceptor logic purges finality signatures that don't match
                // the era validators, so in this case we can continue to
                // use the acceptor
                warn!(
                    "era mismatch from {} for {}; expected: {} and actual: {}",
                    peer, block_hash, expected, actual
                );
                effect_builder.announce_disconnect_from_peer(peer).ignore()
            }
            Err(ref error @ Error::BlockHashMismatch { peer, .. }) => {
                warn!(%error, "finality signature has mismatched block_hash");
                effect_builder.announce_disconnect_from_peer(peer).ignore()
            }
            Err(ref error @ Error::SufficientFinalityWithoutBlock { .. }) => {
                error!(%error, "should not have sufficient finality without block");
                Effects::new()
            }
            Err(Error::InvalidConfiguration) => fatal!(
                effect_builder,
                "node has an invalid configuration, shutting down"
            )
            .ignore(),
        }
    }

    /// Drops all old block acceptors and tracks new local block height;
    /// subsequent attempts to register a block lower than tip will be rejected.
    pub(crate) fn register_local_tip(&mut self, height: u64) {
        self.purge();
        self.local_tip = self.local_tip.into_iter().chain(iter::once(height)).max();
    }

    fn purge(&mut self) {
        // todo!: discuss w/ team if this approach or sth similar is acceptable
        let now = Timestamp::now();
        const PURGE_INTERVAL: u32 = 6 * 60 * 60; // 6 hours
        let mut purged = vec![];
        self.block_acceptors.retain(|k, v| {
            let expired =
                now.saturating_diff(v.last_progress()) > TimeDiff::from_seconds(PURGE_INTERVAL);
            if expired {
                purged.push(*k)
            }
            !expired
        });
        self.block_children
            .retain(|_parent, child| false == purged.contains(child));
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
        REv: From<BlockAccumulatorAnnouncement> + From<BlockCompleteConfirmationRequest> + Send,
    {
        let mut effects = Effects::new();
        if let Some(block) = block {
            effects.extend(effect_builder.mark_block_completed(block.height()).ignore());
            effects.extend(effect_builder.announce_block_accepted(block).ignore());
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

pub(crate) trait ReactorEvent:
    From<StorageRequest>
    + From<PeerBehaviorAnnouncement>
    + From<BlockAccumulatorAnnouncement>
    + From<BlockCompleteConfirmationRequest>
    + From<ControlAnnouncement>
    + Send
    + 'static
{
}

impl<REv> ReactorEvent for REv where
    REv: From<StorageRequest>
        + From<PeerBehaviorAnnouncement>
        + From<BlockAccumulatorAnnouncement>
        + From<BlockCompleteConfirmationRequest>
        + From<ControlAnnouncement>
        + Send
        + 'static
{
}

impl<REv: ReactorEvent> Component<REv> for BlockAccumulator {
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
            Event::ValidatorMatrixUpdated => self.handle_validators(effect_builder),
            Event::ReceivedBlock { block, sender } => {
                self.register_block(effect_builder, *block, Some(sender))
            }
            Event::CreatedFinalitySignature { finality_signature } => {
                self.register_finality_signature(effect_builder, *finality_signature, None)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => {
                self.register_finality_signature(effect_builder, *finality_signature, Some(sender))
            }
            Event::ExecutedBlock { block } => {
                let height = block.header().height();
                let effects = self.register_block(effect_builder, *block, None);
                self.register_local_tip(height);
                effects
            }
            Event::Stored {
                block,
                finality_signatures,
            } => self.handle_stored(effect_builder, block, finality_signatures),
        }
    }
}

impl<REv: ReactorEvent> ValidatorBoundComponent<REv> for BlockAccumulator {
    fn handle_validators(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Self::Event> {
        let mut effects = Effects::new();
        for block_acceptor in self
            .block_acceptors
            .values_mut()
            .filter(|acceptor| false == acceptor.has_sufficient_finality())
        {
            if let Some(era_id) = block_acceptor.era_id() {
                if let Some(evw) = self.validator_matrix.validator_weights(era_id) {
                    effects.extend(store_block_and_finality_signatures(
                        effect_builder,
                        block_acceptor.should_store_block(&evw),
                    ));
                }
            }
        }
        effects
    }
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
