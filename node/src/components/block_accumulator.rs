mod block_acceptor;
mod config;
mod error;
mod event;
mod starting_with;
mod sync_instruction;

use std::{cmp::Ordering, collections::BTreeMap, iter};

use datasize::DataSize;
use futures::FutureExt;
use tracing::{debug, error, warn};

use casper_types::{EraId, TimeDiff, Timestamp};

use crate::{
    components::{small_network::blocklist::BlocklistJustification, Component},
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
pub use error::Error;
pub(crate) use event::Event;
pub(crate) use starting_with::StartingWith;
pub(crate) use sync_instruction::SyncInstruction;

#[derive(Clone, Copy, DataSize, Debug, Eq, PartialEq)]
struct LocalTipIdentifier {
    height: u64,
    era_id: EraId,
}

impl LocalTipIdentifier {
    fn new(height: u64, era_id: EraId) -> Self {
        Self { height, era_id }
    }
}

impl PartialOrd for LocalTipIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.height.partial_cmp(&other.height)
    }
}

impl Ord for LocalTipIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        self.height.cmp(&other.height)
    }
}

/// A cache of pending blocks and finality signatures that are gossiped to this node.
///
/// Announces new blocks and finality signatures once they become valid.
#[derive(DataSize, Debug)]
pub(crate) struct BlockAccumulator {
    /// This component requires the era validator weights for every era
    /// it receives blocks and / or finality signatures for to verify that
    /// the received signatures are legitimate to the era and to calculate
    /// sufficient finality from collected finality signatures.
    validator_matrix: ValidatorMatrix,
    /// Each block_acceptor instance is responsible for combining
    /// potential blocks and their finality signatures. When we have
    /// collected sufficient finality weight's worth of signatures
    /// for a potential block, we accept the block and store it.
    block_acceptors: BTreeMap<BlockHash, BlockAcceptor>,
    /// Key is the parent block hash, value is the child block hash.
    /// Used to determine if we have awareness of the next block to be
    /// sync'd or executed.
    block_children: BTreeMap<BlockHash, BlockHash>,
    /// The height of the subjective local tip of the chain. This is used to
    /// keep track of whether blocks received from the network are relevant or not,
    /// and to determine if this node is close enough to the perceived tip of the
    /// network to transition to executing block for itself.
    local_tip: Option<LocalTipIdentifier>,
    /// Configured setting for how close to perceived tip local tip must be for
    /// this node to attempt block execution for itself.
    attempt_execution_threshold: u64,
    /// Configured setting for tolerating a lack of newly received block
    /// and / or finality signature data. If we last saw progress longer
    /// ago than this interval, we will poll the network to determine
    /// if we are caught up or have become isolated.
    dead_air_interval: TimeDiff,
    /// Configured setting for how often to purge dead state.
    purge_interval: TimeDiff,
    /// Configured setting for how many eras are considered to be recent.
    recent_era_interval: u64,
    /// Tracks activity and assists with perceived tip determination.
    last_progress: Timestamp,
}

impl BlockAccumulator {
    pub(crate) fn new(
        config: Config,
        validator_matrix: ValidatorMatrix,
        local_tip_height_and_era_id: Option<(u64, EraId)>,
        recent_era_interval: u64,
    ) -> Self {
        Self {
            validator_matrix,
            attempt_execution_threshold: config.attempt_execution_threshold(),
            dead_air_interval: config.dead_air_interval(),
            block_acceptors: Default::default(),
            block_children: Default::default(),
            last_progress: Timestamp::now(),
            purge_interval: config.purge_interval(),
            local_tip: local_tip_height_and_era_id
                .map(|(height, era_id)| LocalTipIdentifier::new(height, era_id)),
            recent_era_interval,
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

        if let StartingWith::LocalTip(_, height, era_id) = starting_with {
            self.register_local_tip(height, era_id);
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

    /// Drops all old block acceptors and tracks new local block height;
    /// subsequent attempts to register a block lower than tip will be rejected.
    pub(crate) fn register_local_tip(&mut self, height: u64, era_id: EraId) {
        self.purge();
        self.local_tip = self
            .local_tip
            .into_iter()
            .chain(iter::once(LocalTipIdentifier::new(height, era_id)))
            .max();
    }

    fn register_peer(
        &mut self,
        block_hash: BlockHash,
        maybe_era_id: Option<EraId>,
        sender: NodeId,
    ) {
        let maybe_local_tip_era_id = self.local_tip.map(|local_tip| local_tip.era_id);
        match (maybe_era_id, maybe_local_tip_era_id) {
            // When the era of the item sent by this peer is known, we check it
            // against our local tip era. If it is recent (within a number of
            // eras of our local tip), we create an acceptor for it along with
            // registering the peer.
            (Some(era_id), Some(local_tip_era_id))
                if era_id >= local_tip_era_id.saturating_sub(self.recent_era_interval) =>
            {
                let acceptor = self
                    .block_acceptors
                    .entry(block_hash)
                    .or_insert_with(|| BlockAcceptor::new(block_hash, None));
                acceptor.register_peer(sender);
            }
            // In all other cases (i.e. the item's era is not provided, the
            // local tip doesn't have an era or the item's era is older than
            // the local tip era by more than `recent_era_interval`), we only
            // register the peer if there is an acceptor for the item already.
            _ => {
                if let Some(acceptor) = self.block_acceptors.get_mut(&block_hash) {
                    acceptor.register_peer(sender)
                }
            }
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
            .as_ref()
            .map_or(false, |local_tip| block_height < local_tip.height)
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
            .or_insert_with(|| BlockAcceptor::new(*block_hash, None));

        match acceptor.register_block(block, sender) {
            Ok(_) => match self.validator_matrix.validator_weights(era_id) {
                Some(evw) => store_block_and_finality_signatures(
                    effect_builder,
                    acceptor.should_store_block(&evw),
                ),
                None => Effects::new(),
            },
            Err(error) => match error {
                Error::InvalidGossip(ref gossip_error) => {
                    warn!(%gossip_error, "received invalid block");
                    effect_builder
                        .announce_block_peer_with_justification(
                            gossip_error.peer(),
                            BlocklistJustification::SentBadBlock { error },
                        )
                        .ignore()
                }
                Error::EraMismatch {
                    peer,
                    block_hash,
                    expected,
                    actual,
                } => {
                    warn!(
                        "era mismatch from {} for {}; expected: {} and actual: {}",
                        peer, block_hash, expected, actual
                    );
                    effect_builder
                        .announce_block_peer_with_justification(
                            peer,
                            BlocklistJustification::SentBadBlock { error },
                        )
                        .ignore()
                }
                ref error @ Error::BlockHashMismatch { .. } => {
                    error!(%error, "finality signature has mismatched block_hash; this is a bug");
                    Effects::new()
                }
                ref error @ Error::SufficientFinalityWithoutBlock { .. } => {
                    error!(%error, "should not have sufficient finality without block");
                    Effects::new()
                }
                Error::InvalidConfiguration => fatal!(
                    effect_builder,
                    "node has an invalid configuration, shutting down"
                )
                .ignore(),
                Error::BogusValidator(_) => {
                    error!(%error, "unexpected detection of bogus validator, this is a bug");
                    Effects::new()
                }
            },
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
        let block_hash = finality_signature.block_hash;
        let era_id = finality_signature.era_id;
        let maybe_local_tip_era_id = self.local_tip.map(|local_tip| local_tip.era_id);
        let acceptor = match maybe_local_tip_era_id {
            // When the era of the finality signature being registered is
            // known, we check it against our local tip era. If it is recent
            // (within a number of eras of our local tip), we create an
            // acceptor for it if one is not already present before registering
            // the finality signature.
            Some(local_tip_era_id)
                if era_id >= local_tip_era_id.saturating_sub(self.recent_era_interval) =>
            {
                self.block_acceptors
                    .entry(block_hash)
                    .or_insert_with(|| BlockAcceptor::new(block_hash, sender))
            }
            // In all other cases (i.e. the local tip doesn't have an era or
            // the signature's era is older than the local tip era by more than
            // `recent_era_interval`), we only register the signature if there
            // is an acceptor for it already.
            _ => match self.block_acceptors.get_mut(&block_hash) {
                Some(acceptor) => acceptor,
                // When there is no acceptor for it, this function returns
                // early, ignoring the signature.
                None => return Effects::new(),
            },
        };

        match acceptor.register_finality_signature(finality_signature, sender) {
            Ok(Some(finality_signature)) => store_block_and_finality_signatures(
                effect_builder,
                (ShouldStore::SingleSignature(finality_signature), None),
            ),
            Ok(None) => match &self.validator_matrix.validator_weights(era_id) {
                Some(evw) => store_block_and_finality_signatures(
                    effect_builder,
                    acceptor.should_store_block(evw),
                ),
                None => Effects::new(),
            },

            Err(error) => {
                match error {
                    Error::InvalidGossip(ref gossip_error) => {
                        warn!(%gossip_error, "received invalid finality_signature");
                        effect_builder
                            .announce_block_peer_with_justification(
                                gossip_error.peer(),
                                BlocklistJustification::SentBadFinalitySignature { error },
                            )
                            .ignore()
                    }
                    Error::EraMismatch {
                        peer,
                        block_hash,
                        expected,
                        actual,
                    } => {
                        // the acceptor logic purges finality signatures that don't match
                        // the era validators, so in this case we can continue to
                        // use the acceptor
                        warn!(
                            "era mismatch from {} for {}; expected: {} and actual: {}",
                            peer, block_hash, expected, actual
                        );
                        effect_builder
                            .announce_block_peer_with_justification(
                                peer,
                                BlocklistJustification::SentBadFinalitySignature { error },
                            )
                            .ignore()
                    }
                    ref error @ Error::BlockHashMismatch { .. } => {
                        error!(%error, "finality signature has mismatched block_hash; this is a bug");
                        Effects::new()
                    }
                    ref error @ Error::SufficientFinalityWithoutBlock { .. } => {
                        error!(%error, "should not have sufficient finality without block");
                        Effects::new()
                    }
                    Error::InvalidConfiguration => fatal!(
                        effect_builder,
                        "node has an invalid configuration, shutting down"
                    )
                    .ignore(),
                    Error::BogusValidator(_) => {
                        error!(%error, "unexpected detection of bogus validator, this is a bug");
                        Effects::new()
                    }
                }
            }
        }
    }

    fn register_stored<REv>(
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

    fn highest_usable_block_height(&mut self) -> Option<u64> {
        let mut ret: Option<u64> = self.local_tip.map(|local_tip| local_tip.height);
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
            .map(|acceptor| acceptor.peers().iter().cloned().collect())
    }

    fn should_sync(&mut self, starting_with_block_height: u64) -> bool {
        match self.highest_usable_block_height() {
            Some(highest_usable_block_height) => {
                let height_diff =
                    highest_usable_block_height.saturating_sub(starting_with_block_height);
                height_diff <= self.attempt_execution_threshold
            }
            None => false,
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

    fn purge(&mut self) {
        let now = Timestamp::now();
        let mut purged = vec![];
        let purge_interval = self.purge_interval;
        self.block_acceptors.retain(|k, v| {
            let expired = now.saturating_diff(v.last_progress()) > purge_interval;
            if expired {
                purged.push(*k)
            }
            !expired
        });
        self.block_children
            .retain(|_parent, child| false == purged.contains(child));
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
            Event::RegisterPeer {
                block_hash,
                era_id,
                sender,
            } => {
                self.register_peer(block_hash, era_id, sender);
                Effects::new()
            }
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
                let era_id = block.header().era_id();
                let effects = self.register_block(effect_builder, *block, None);
                self.register_local_tip(height, era_id);
                effects
            }
            Event::Stored {
                block,
                finality_signatures,
            } => self.register_stored(effect_builder, block, finality_signatures),
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

fn store_block_and_finality_signatures<REv, I>(
    effect_builder: EffectBuilder<REv>,
    (should_store, faulty_senders): (ShouldStore, I),
) -> Effects<Event>
where
    REv: From<PeerBehaviorAnnouncement> + From<StorageRequest> + Send,
    I: IntoIterator<Item = (NodeId, Error)>,
{
    let mut effects = match should_store {
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
    };
    effects.extend(faulty_senders.into_iter().flat_map(|(node_id, error)| {
        effect_builder
            .announce_block_peer_with_justification(
                node_id,
                BlocklistJustification::SentBadFinalitySignature { error },
            )
            .ignore()
    }));
    effects
}
