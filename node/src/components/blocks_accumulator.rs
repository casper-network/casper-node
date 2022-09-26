mod block_gossip_acceptor;
mod error;
mod event;

use std::{
    collections::{btree_map::Entry, BTreeMap},
    hash::Hash,
};

use casper_types::system::auction::ValidatorWeights;
use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use tracing::{debug, error, warn};

use casper_types::{EraId, PublicKey, Timestamp};

use crate::{
    components::Component,
    effect::{announcements::PeerBehaviorAnnouncement, EffectBuilder, EffectExt, Effects},
    types::{
        Block, BlockAdded, BlockHash, EraValidatorWeights, FetcherItem, FinalitySignature, Item,
        NodeId, SignatureWeight, ValidatorMatrix,
    },
    NodeRng,
};

use crate::types::BlockHeader;
use block_gossip_acceptor::BlockGossipAcceptor;
use error::Error;
pub(crate) use event::Event;

/// A cache of pending blocks and finality signatures that are gossiped to this node.
///
/// Announces new blocks and finality signatures once they become valid.
#[derive(DataSize, Debug)]
pub(crate) struct BlocksAccumulator {
    filter: Vec<BlockHash>,
    block_gossip_acceptors: BTreeMap<BlockHash, BlockGossipAcceptor>,
    block_children: BTreeMap<BlockHash, BlockHash>,
    validator_matrix: ValidatorMatrix,
}

pub(crate) enum SyncInstruction {
    Leap,
    CaughtUp,
    BlockSync {
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
    },
    BlockExec {
        block: Box<Block>,
    },
}

pub(crate) enum StartingWith {
    Block(Box<Block>),
    Hash(BlockHash),
    // simplifies call sites; results in a Leap instruction
    Nothing,
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            StartingWith::Block(block) => block.id(),
            StartingWith::Hash(hash) => *hash,
            StartingWith::Nothing => BlockHash::default(),
        }
    }

    pub(crate) fn have_block(&self) -> bool {
        match self {
            // StartingWith::Block means we have have the block locally and
            // it should have global state
            StartingWith::Block(_) => true,
            StartingWith::Hash(_) => false,
            StartingWith::Nothing => false,
        }
    }
}

impl BlocksAccumulator {
    pub(crate) fn new(validator_matrix: ValidatorMatrix) -> Self {
        Self {
            filter: Default::default(),
            block_gossip_acceptors: Default::default(),
            block_children: Default::default(),
            validator_matrix,
        }
    }

    pub(crate) fn flush(mut self, validator_matrix: ValidatorMatrix) -> Self {
        Self {
            filter: Default::default(),
            block_gossip_acceptors: Default::default(),
            block_children: Default::default(),
            validator_matrix,
        }
    }

    pub(crate) fn flush_filter(&mut self) {
        self.filter.clear();
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

        const ATTEMPT_EXECUTION_THRESHOLD: u64 = 3; // TODO: make chainspec or cfg setting

        let should_fetch_execution_state = starting_with.have_block() == false;
        let block_hash = starting_with.block_hash();
        if let Some((highest_block_hash, highest_block_height, highest_era_id)) =
            self.highest_known_block()
        {
            let (era_id, block_height) = match starting_with {
                StartingWith::Nothing => {
                    return SyncInstruction::Leap;
                }
                StartingWith::Block(block) => (block.header().era_id(), block.header().height()),
                StartingWith::Hash(trusted_hash) => {
                    match self.block_gossip_acceptors.get(&trusted_hash) {
                        None => {
                            // the accumulator is unaware of the starting-with block
                            return SyncInstruction::Leap;
                        }
                        Some(gossiped_block) => {
                            match gossiped_block.block_era_and_height() {
                                None => {
                                    // we have received at least one finality signature for this
                                    // block via gossiping but
                                    // have not seen the block body itself
                                    return SyncInstruction::Leap;
                                }
                                // we can derive the height for the trusted hash
                                // because we've seen the block it refers to via gossiping
                                Some(block_era_and_height) => block_era_and_height,
                            }
                        }
                    }
                }
            };

            // the starting-with block may be close to perceived tip
            let height_diff = highest_block_height.saturating_sub(block_height);
            if height_diff == 0 {
                // TODO: maybe add some time slippage awareness re: the last time
                // we heard anything via gossiping, timestamp on the highest block
                // we currently know about, etc
                return SyncInstruction::CaughtUp;
            }
            if height_diff <= ATTEMPT_EXECUTION_THRESHOLD {
                if let Some(child_hash) = self.block_children.get(&block_hash) {
                    if let Some(block_acceptor) = self.block_gossip_acceptors.get_mut(child_hash) {
                        if let Some(block) = block_acceptor.executable_block() {
                            return SyncInstruction::BlockExec {
                                block: Box::new(block),
                            };
                        }
                    }

                    return SyncInstruction::BlockSync {
                        block_hash: *child_hash,
                        should_fetch_execution_state,
                    };
                }
            }
        }
        SyncInstruction::Leap
    }

    pub(crate) fn register_block_by_identifier<REv>(
        &mut self,
        block_hash: BlockHash,
        era_id: EraId,
    ) {
        if self.filter.contains(&block_hash) {
            return;
        }
        let mut acceptor = BlockGossipAcceptor::new(block_hash);
        if let Some(evw) = self.validator_matrix.validator_weights(era_id) {
            if let Err(err) = acceptor.register_era_validator_weights(evw) {
                warn!(%err, "unable to register era_validator_weights");
                return;
            }
        }
        self.block_gossip_acceptors.insert(block_hash, acceptor);
    }

    pub(crate) fn register_block_added<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block_added: BlockAdded,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: Send + From<PeerBehaviorAnnouncement>,
    {
        let block_hash = *block_added.block.hash();
        let era_id = block_added.block.header().era_id();
        if let Some(parent_hash) = block_added.block.parent() {
            self.block_children.insert(*parent_hash, block_hash);
        }
        let acceptor = match self.get_or_register_acceptor_mut(block_hash, era_id) {
            Some(block_gossip_acceptor) => block_gossip_acceptor,
            None => {
                return Effects::new();
            }
        };

        if let Err(err) = acceptor.register_block_added(block_added, sender) {
            warn!(%err, "received invalid block_added");
            match err {
                Error::InvalidGossip(err) => {
                    return effect_builder
                        .announce_disconnect_from_peer(err.peer())
                        .ignore();
                }
                Error::EraMismatch(err) => {
                    // this block acceptor is borked; get rid of it
                    self.block_gossip_acceptors.remove(&block_hash);
                }
                Error::DuplicatedEraValidatorWeights { .. } => {
                    // this should be unreachable; definitely a programmer error
                    debug!(%err, "unexpected error registering block added");
                }
            }
        }
        Effects::new()
    }

    pub(crate) fn register_finality_signature<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finality_signature: FinalitySignature,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: Send + From<PeerBehaviorAnnouncement>,
    {
        let block_hash = finality_signature.block_hash;
        let era_id = finality_signature.era_id;

        let acceptor = match self.get_or_register_acceptor_mut(block_hash, era_id) {
            Some(block_gossip_acceptor) => block_gossip_acceptor,
            None => {
                return Effects::new();
            }
        };

        if let Err(err) = acceptor.register_finality_signature(finality_signature, sender) {
            warn!(%err, "received invalid finality_signature");
            match err {
                Error::InvalidGossip(err) => {
                    return effect_builder
                        .announce_disconnect_from_peer(err.peer())
                        .ignore();
                }
                Error::EraMismatch(err) => {
                    // the acceptor logic purges finality signatures that don't match
                    // the era validators, so in this case we can continue to
                    // use the acceptor
                    warn!(%err, "finality signature has mismatched era_id");
                }
                Error::DuplicatedEraValidatorWeights { .. } => {
                    // this should be unreachable; definitely a programmer error
                    debug!(%err, "unexpected error registering a finality signature");
                }
            }
        }
        Effects::new()
    }

    pub(crate) fn register_updated_validator_matrix(&mut self, validator_matrix: ValidatorMatrix) {
        self.validator_matrix = validator_matrix;
        let block_hashes = self.block_gossip_acceptors.keys().copied().collect_vec();
        for block_hash in block_hashes {
            if let Some(mut acceptor) = self.block_gossip_acceptors.remove(&block_hash) {
                if let Some(era_id) = acceptor.era_id() {
                    if let Some(evw) = self.validator_matrix.validator_weights(era_id) {
                        acceptor = acceptor.refresh(evw);
                    }
                }
                self.block_gossip_acceptors.insert(block_hash, acceptor);
            }
        }
    }

    pub(crate) fn register_new_local_tip(&mut self, block_header: &BlockHeader) {
        self.block_gossip_acceptors.remove(&block_header.hash());
        self.filter.push(block_header.hash())
    }

    pub(crate) fn last_progress(&self) -> Option<Timestamp> {
        self.block_gossip_acceptors
            .iter()
            .max_by(|(xk, xv), (yk, yv)| xv.last_progress().cmp(&yv.last_progress()))
            .map(|(_, bga)| bga.last_progress())
    }

    fn highest_known_block(&self) -> Option<(BlockHash, u64, EraId)> {
        let mut ret: Option<(BlockHash, u64, EraId)> = None;
        for (k, v) in &self.block_gossip_acceptors {
            if self.filter.contains(k) {
                // should be unreachable
                continue;
            }
            if v.can_execute() == false {
                continue;
            }
            match v.block_era_and_height() {
                None => {
                    continue;
                }
                Some((era_id, height)) => {
                    if let Some((bh, h, e)) = ret {
                        if height <= h {
                            continue;
                        }
                    }
                    ret = Some((*k, height, era_id));
                }
            };
        }
        ret
    }

    fn get_or_register_acceptor_mut(
        &mut self,
        block_hash: BlockHash,
        era_id: EraId,
    ) -> Option<&mut BlockGossipAcceptor> {
        if let Entry::Occupied(mut entry) = self.block_gossip_acceptors.entry(block_hash) {
            if self.filter.contains(&block_hash) {
                return None;
            }
            if let Some(evw) = self.validator_matrix.validator_weights(era_id) {
                let acceptor = BlockGossipAcceptor::new_with_validator_weights(block_hash, evw);
                entry.insert(acceptor);
            } else {
                entry.insert(BlockGossipAcceptor::new(block_hash));
            }
        }

        self.block_gossip_acceptors.get_mut(&block_hash)
    }
}

// TODO: is this even really a component?
impl<REv> Component<REv> for BlocksAccumulator
where
    REv: Send + From<PeerBehaviorAnnouncement>,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::ReceivedBlock { block, sender } => {
                self.register_block_added(effect_builder, *block, sender)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => self.register_finality_signature(effect_builder, *finality_signature, sender),
            Event::UpdatedValidatorMatrix { era_id } => {
                //self.handle_updated_validator_matrix(effect_builder, era_id)
                Effects::new()
            }
        }
    }
}
