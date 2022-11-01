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
    types::{Block, BlockHash, BlockSignatures, FinalitySignature, NodeId, ValidatorMatrix},
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
        let block_hash = starting_with.block_hash();
        let block_height = match starting_with.block_height() {
            Some(height) => height,
            None => {
                let maybe_height = {
                    if let Some(block_acceptor) = self.block_acceptors.get(&block_hash) {
                        block_acceptor.block_height()
                    } else {
                        None
                    }
                };
                match maybe_height {
                    Some(height) => height,
                    None => {
                        // we have no height for this block hash, so we must leap
                        return SyncInstruction::Leap { block_hash };
                    }
                }
            }
        };

        // BEFORE the f-seq cant help you, LEAP
        // ? |------------- future chain ------------------------>
        // IN f-seq not in range of tip, LEAP
        // |------------- future chain ----?-ATTEMPT_EXECUTION_THRESHOLD->
        // IN f-seq in range of tip, CAUGHT UP (which will ultimately result in EXEC)
        // |------------- future chain ----?ATTEMPT_EXECUTION_THRESHOLD>
        // AFTER the f-seq cant help you, SYNC-all-state
        // |------------- future chain ------------------------> ?
        if starting_with.is_executable_block() {
            let next_block_hash = {
                let mut ret: Option<BlockHash> = None;
                if let Some(highest_perceived) = self.highest_usable_block_height() {
                    debug_assert!(
                        block_height <= highest_perceived,
                        "executable block height({}) is higher than highest perceived({})",
                        block_height,
                        highest_perceived
                    );
                    if highest_perceived.saturating_sub(self.attempt_execution_threshold)
                        <= block_height
                    {
                        // this will short circuit the block synchronizer to start syncing the
                        // child of this block after enqueueing this block for execution
                        ret = self.next_syncable_block_hash(block_hash)
                    }
                }
                ret
            };
            return SyncInstruction::BlockExec {
                block_hash,
                next_block_hash,
            };
        }

        if let StartingWith::LocalTip {
            block_hash,
            block_height,
            is_last_block_before_activation,
        } = starting_with
        {
            self.register_local_tip(block_height);
            if is_last_block_before_activation {
                return SyncInstruction::Leap { block_hash };
            }
        }

        if self.should_sync(block_height) {
            let block_hash_to_sync = match (
                starting_with.is_synced_block_identifier(),
                self.next_syncable_block_hash(block_hash),
            ) {
                (true, None) => {
                    // the block we just finished syncing appears to have no perceived children
                    // and is either at tip or within execution range of tip
                    None
                }
                (true, Some(child_hash)) => Some(child_hash),
                (false, _) => Some(block_hash),
            };

            match block_hash_to_sync {
                Some(block_hash) => {
                    self.last_progress = Timestamp::now();
                    return SyncInstruction::BlockSync { block_hash };
                }
                None => {
                    // we expect to be receiving gossiped blocks from other nodes
                    // if we haven't received any messages describing higher blocks
                    // for more than the self.dead_air_interval config allows
                    // we leap again to poll the network
                    if self.last_progress.elapsed() < self.dead_air_interval {
                        return SyncInstruction::CaughtUp;
                    }
                    // we don't want to swamp the network with "are we there yet" leaps.
                    self.last_progress = Timestamp::now();
                }
            }
        }
        SyncInstruction::Leap { block_hash }
    }

    fn should_sync(&self, starting_with_block_height: u64) -> bool {
        match self.highest_usable_block_height() {
            Some(highest_usable_block_height) => {
                let height_diff =
                    highest_usable_block_height.saturating_sub(starting_with_block_height);
                height_diff <= self.attempt_execution_threshold
            }
            None => false,
        }
    }

    fn next_syncable_block_hash(&self, parent_block_hash: BlockHash) -> Option<BlockHash> {
        let child_hash = self.block_children.get(&parent_block_hash)?;
        let block_acceptor = self.block_acceptors.get(child_hash)?;
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
        // todo!: Also register local tip's era ID; for older signatures, don't create acceptor.

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
        let now = Timestamp::now();
        const PURGE_INTERVAL: u32 = 6 * 60 * 60; // 6 hours todo!("move to config")
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

    fn highest_usable_block_height(&self) -> Option<u64> {
        let mut ret: Option<u64> = self.local_tip;
        for block_acceptor in &mut self.block_acceptors.values() {
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
