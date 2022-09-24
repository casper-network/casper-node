mod block_acceptor;
mod error;
mod event;

use std::{
    collections::{btree_map::Entry, BTreeMap},
    hash::Hash,
};

use casper_types::system::auction::ValidatorWeights;
use datasize::DataSize;
use num_rational::Ratio;
use tracing::{error, warn};

use casper_types::{EraId, PublicKey};

use crate::{
    components::Component,
    effect::{announcements::BlocklistAnnouncement, EffectBuilder, EffectExt, Effects},
    types::{
        Block, BlockAdded, BlockHash, FetcherItem, FinalitySignature, NodeId, SignatureWeight,
        ValidatorMatrix,
    },
    NodeRng,
};

use block_acceptor::BlockAcceptor;
use error::Error;
pub(crate) use event::Event;

// #[derive(Debug)]
// enum SignaturesFinality {
//     Sufficient,
//     NotSufficient,
//     BogusValidators(Vec<PublicKey>),
// }

/// A cache of pending blocks and finality signatures that are gossiped to this node.
///
/// Announces new blocks and finality signatures once they become valid.
#[derive(DataSize, Debug)]
pub(crate) struct BlocksAccumulator {
    block_acceptors: BTreeMap<BlockHash, BlockAcceptor>,
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
}

pub(crate) enum StartingWith {
    Block(Box<Block>),
    Hash(BlockHash),
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> &BlockHash {
        match self {
            StartingWith::Block(block) => block.as_ref().hash(),
            StartingWith::Hash(hash) => hash,
        }
    }
}

impl BlocksAccumulator {
    pub(crate) fn new(validator_matrix: ValidatorMatrix) -> Self {
        Self {
            block_acceptors: Default::default(),
            block_children: Default::default(),
            validator_matrix,
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

        const ATTEMPT_EXECUTION_THRESHOLD: u64 = 3; // TODO: make chainspec or cfg setting

        let block_hash = *starting_with.block_hash();
        if let Some((highest_block_hash, highest_block_height, highest_era_id)) =
            self.highest_known_block()
        {
            let block_height = match starting_with {
                StartingWith::Block(block) => block.header().height(),
                StartingWith::Hash(trusted_hash) => match self.block_acceptors.get(&trusted_hash) {
                    None => {
                        // the accumulator is unaware of the starting-with block
                        return SyncInstruction::Leap;
                    }
                    Some(block_acceptor) => match block_acceptor.block_height() {
                        None => {
                            return SyncInstruction::Leap;
                        }
                        Some(block_height) => block_height,
                    },
                },
            };

            let height_diff = highest_block_height.saturating_sub(block_height);
            if height_diff <= ATTEMPT_EXECUTION_THRESHOLD {
                if let Some(child_hash) = self.block_children.get(&block_hash) {
                    if let Some(block_acceptor) = self.block_acceptors.get_mut(child_hash) {
                        match self.validator_matrix.validator_weights(highest_era_id) {
                            None => return SyncInstruction::Leap,
                            Some(_) => {
                                // TODO: we need to make sure we wait to get enuff finality
                                // signatures
                                if block_acceptor.can_execute() {
                                    return SyncInstruction::CaughtUp;
                                }
                            }
                        };
                    }

                    // not within range of tip, but we have a likely block to attempt
                    // state acquisition for
                    let should_fetch_execution_state: bool = true; // figure this bit out
                    return SyncInstruction::BlockSync {
                        block_hash: *child_hash,
                        should_fetch_execution_state,
                    };
                }
            }
        }
        SyncInstruction::Leap
    }

    fn handle_block_added<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block_added: BlockAdded,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: Send + From<BlocklistAnnouncement>,
    {
        // TODO
        // * check if we have the deploys - if we do, add the finalized approvals to storage
        // * if missing deploy, fetch deploy and store

        let block_hash = *block_added.block.hash();
        if let Some(parent_hash) = block_added.block.parent() {
            self.block_children.insert(*parent_hash, block_hash);
        }
        let era_id = block_added.block.header().era_id();
        let can_execute = match self.block_acceptors.entry(block_hash) {
            Entry::Vacant(entry) => {
                if let Err(err) = block_added.validate(&()) {
                    warn!(%err, "received invalid block");
                    return effect_builder
                        .announce_disconnect_from_peer(sender)
                        .ignore();
                }
                match BlockAcceptor::new_from_block_added(
                    block_added,
                    self.validator_matrix.validator_weights(era_id),
                ) {
                    Ok(block_acceptor) => entry.insert(block_acceptor),
                    Err(_error) => {
                        return effect_builder
                            .announce_disconnect_from_peer(sender)
                            .ignore();
                    }
                };
                false
            }
            Entry::Occupied(entry) => {
                let accumulated_block = entry.into_mut();
                if let Err(_error) = accumulated_block.register_block(block_added) {
                    return Effects::new();
                }
                accumulated_block.can_execute()
            }
        };

        if can_execute {
            if let Some(accumulated_block) = self.block_acceptors.remove(&block_hash) {
                self.announce(effect_builder, &accumulated_block)
            } else {
                error!(%block_hash, "should have block acceptor for block");
                Effects::new()
            }
        } else {
            Effects::new()
        }

        // TODO: Check for duplicated block and given height from given sender.
    }

    fn handle_finality_signature<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finality_signature: FinalitySignature,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: Send + From<BlocklistAnnouncement>,
    {
        // if let Err(error) = finality_signature.is_signer_in_era() {
        //     warn!(%error, "received finality signature from a non-validator");
        //     return effect_builder
        //         .announce_disconnect_from_peer(sender)
        //         .ignore();
        // }

        if let Err(error) = finality_signature.is_verified() {
            warn!(%error, %sender, %finality_signature, "received invalid finality signature");
            return effect_builder
                .announce_disconnect_from_peer(sender)
                .ignore();
        }

        let block_hash = finality_signature.block_hash;
        let bogus_validators = self.validator_matrix.bogus_validators(
            finality_signature.era_id,
            std::iter::once(&finality_signature.public_key),
        );
        match bogus_validators {
            Some(bogus_validators) if !bogus_validators.is_empty() => {
                warn!(%sender, %finality_signature, "bogus validator");
                return effect_builder
                    .announce_disconnect_from_peer(sender)
                    .ignore();
            }
            Some(_) | None => (),
        }

        let can_execute = match self.block_acceptors.entry(block_hash) {
            Entry::Vacant(entry) => {
                let era_id = finality_signature.era_id;
                match BlockAcceptor::new_from_finality_signature(
                    finality_signature,
                    self.validator_matrix.validator_weights(era_id),
                ) {
                    Ok(block_acceptor) => entry.insert(block_acceptor),
                    Err(_error) => {
                        return effect_builder
                            .announce_disconnect_from_peer(sender)
                            .ignore()
                    }
                };
                false
            }
            Entry::Occupied(entry) => {
                let accumulated_block = entry.into_mut();
                if accumulated_block
                    .register_signature(finality_signature)
                    .is_err()
                {
                    return effect_builder
                        .announce_disconnect_from_peer(sender)
                        .ignore();
                };
                accumulated_block.can_execute()
            }
        };

        if can_execute {
            if let Some(accumulated_block) = self.block_acceptors.remove(&block_hash) {
                self.announce(effect_builder, &accumulated_block)
            } else {
                error!(%block_hash, "should have block acceptor for block");
                Effects::new()
            }
        } else {
            Effects::new()
        }

        // TODO: Special care for a switch block?

        // - proceed to execute
        // - store block
        // - remove from block_acceptors
        //}

        // TODO: Reject if the era is out of the range we currently handle.
        // TODO: Limit the number of items per peer.
    }

    fn handle_updated_validator_matrix<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        era_id: EraId,
    ) -> Effects<Event> {
        for block_acceptor in self.block_acceptors.values_mut() {
            block_acceptor.remove_bogus_validators(&self.validator_matrix);
            // TODO: Disconnect from the peers who gave us the bogus validators.
        }

        // TODO - announce any blocks which can now be executed?

        // TODO - handle announcing new validator sets

        Effects::new()
    }

    // fn block_height(&self, block_hash: &BlockHash) -> Option<u64> {
    //     for (height, node_id_to_block) in self.pending_blocks {
    //         if node_id_to_block
    //             .iter()
    //             .any(|(node_id, block)| block.hash() == block_hash)
    //         {
    //             return Some(height);
    //         }
    //     }
    //     None
    // }

    fn announce<REv>(
        &self,
        effect_builder: EffectBuilder<REv>,
        accumulated_block: &BlockAcceptor,
    ) -> Effects<Event>
    where
        REv: Send,
    {
        todo!()
    }

    fn highest_known_block(&self) -> Option<(BlockHash, u64, EraId)> {
        let mut ret: Option<(BlockHash, u64, EraId)> = None;
        for (next_key, next_value) in self
            .block_acceptors
            .iter()
            .filter(|(next_key, next_value)| next_value.has_block_added())
        {
            ret = match next_value.block_height() {
                Some(next_height) => match ret {
                    Some((_, curr_height, _curr_era_id)) => {
                        if next_height > curr_height {
                            Some((*next_key, next_height, next_value.era_id()))
                        } else {
                            ret
                        }
                    }
                    None => Some((*next_key, next_height, next_value.era_id())),
                },
                None => ret,
            }
        }
        ret
    }

    fn highest_executable_block(&mut self) -> Option<(BlockHash, u64)> {
        let mut ret: Option<(BlockHash, u64)> = None;
        for (next_key, next_value) in self.block_acceptors.iter_mut() {
            if !next_value.can_execute() {
                continue;
            }
            ret = match next_value.block_height() {
                Some(next_height) => match ret {
                    Some((_, curr_height)) => {
                        if next_height > curr_height {
                            Some((*next_key, next_height))
                        } else {
                            ret
                        }
                    }
                    None => Some((*next_key, next_height)),
                },
                None => ret,
            }
        }
        ret
    }
}

impl<REv> Component<REv> for BlocksAccumulator
where
    REv: Send + From<BlocklistAnnouncement>,
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
                self.handle_block_added(effect_builder, *block, sender)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => self.handle_finality_signature(effect_builder, *finality_signature, sender),
            Event::UpdatedValidatorMatrix { era_id } => {
                self.handle_updated_validator_matrix(effect_builder, era_id)
            }
        }
    }
}
