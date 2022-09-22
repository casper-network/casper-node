mod block_acceptor;
mod error;
mod event;

use std::collections::{btree_map::Entry, BTreeMap};
use std::hash::Hash;

use casper_types::system::auction::ValidatorWeights;
use datasize::DataSize;
use num_rational::Ratio;
use tracing::{error, warn};

use casper_types::{EraId, PublicKey};

use crate::{
    components::Component,
    effect::{announcements::BlocklistAnnouncement, EffectBuilder, EffectExt, Effects},
    types::{Block, BlockAdded, BlockHash, FetcherItem, FinalitySignature, NodeId},
    NodeRng,
};

use block_acceptor::BlockAcceptor;
use error::Error;
pub(crate) use event::Event;

#[derive(Debug)]
enum SignaturesFinality {
    Sufficient,
    NotSufficient,
    BogusValidators(Vec<PublicKey>),
}

/// A cache of pending blocks and finality signatures that are gossiped to this node.
///
/// Announces new blocks and finality signatures once they become valid.
#[derive(DataSize, Debug)]
pub(crate) struct BlocksAccumulator {
    block_acceptors: BTreeMap<BlockHash, BlockAcceptor>,
    block_parents: BTreeMap<BlockHash, BlockHash>,
    #[data_size(skip)]
    fault_tolerance_fraction: Ratio<u64>,
    validator_sets: BTreeMap<EraId, ValidatorWeights>,
}

pub(crate) enum LeapInstruction {
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
    pub(crate) fn new(fault_tolerance_fraction: Ratio<u64>) -> Self {
        Self {
            block_acceptors: Default::default(),
            block_parents: Default::default(),
            fault_tolerance_fraction,
            validator_sets: BTreeMap::new(),
        }
    }

    pub(crate) fn should_leap(&self, starting_with: StartingWith) -> LeapInstruction {
        const ATTEMPT_EXECUTION_THRESHOLD: u64 = 3; // TODO: make chainspec or cfg setting
        let validators = &BTreeMap::new(); // TODO: really gotta get this dealt with.

        let block_hash = *starting_with.block_hash();
        if let Some((highest_block_hash, highest_block_height)) = self.highest_known_block() {
            let block_height = match starting_with {
                StartingWith::Block(block) => block.header().height(),
                StartingWith::Hash(trusted_hash) => match self.block_acceptors.get(&trusted_hash) {
                    None => {
                        // the accumulator is unaware of the starting-with block
                        return LeapInstruction::Leap;
                    }
                    Some(block_acceptor) => match block_acceptor.block_height() {
                        None => {
                            // the accumulator doesn't know the height of the starting-with block
                            // this means we've seen one or more signatures for the block but
                            // have not seen the block added message for it.
                            // under normal circumstances this means we're close to the tip
                            // but it is possible we'll never see the BlockAdded via
                            // gossiping
                            return LeapInstruction::BlockSync {
                                block_hash: trusted_hash,
                                should_fetch_execution_state: true,
                            };
                        }
                        Some(block_height) => block_height,
                    },
                },
            };

            let height_diff = highest_block_height.saturating_sub(block_height);
            if height_diff <= ATTEMPT_EXECUTION_THRESHOLD {
                if let Some(child_hash) = self.block_parents.get(&block_hash) {
                    if let Some(block_acceptor) = self.block_acceptors.get(child_hash) {
                        if block_acceptor.can_execute(self.fault_tolerance_fraction, validators) {
                            return LeapInstruction::CaughtUp;
                        }
                    }
                    return LeapInstruction::BlockSync {
                        block_hash: *child_hash,
                        should_fetch_execution_state: false,
                    };
                }
            }
        }
        LeapInstruction::Leap
    }

    pub(crate) fn can_execute(&self, block_hash: &BlockHash) -> bool {
        todo!("use block acceptors to determine if execution can be attempted or not");
        false
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
            self.block_parents.insert(*parent_hash, block_hash);
        }
        let era_id = block_added.block.header().era_id();
        let has_sufficient_signatures = match self.block_acceptors.entry(block_hash) {
            Entry::Vacant(entry) => {
                if let Err(err) = block_added.validate(&()) {
                    warn!(%err, "received invalid block");
                    return effect_builder
                        .announce_disconnect_from_peer(sender)
                        .ignore();
                }
                match BlockAcceptor::new_from_block_added(block_added) {
                    Ok(block_acceptor) => entry.insert(block_acceptor),
                    Err(_error) => {
                        return effect_builder
                            .announce_disconnect_from_peer(sender)
                            .ignore();
                    }
                };
                SignaturesFinality::NotSufficient
            }
            Entry::Occupied(entry) => {
                let accumulated_block = entry.into_mut();
                if let Err(_error) = accumulated_block.register_block(block_added) {
                    return Effects::new();
                }
                match self.validator_sets.get(&era_id) {
                    Some(validators) => accumulated_block
                        .has_sufficient_signatures(self.fault_tolerance_fraction, validators),
                    None => SignaturesFinality::NotSufficient,
                }
            }
        };

        match has_sufficient_signatures {
            SignaturesFinality::Sufficient => {
                if let Some(accumulated_block) = self.block_acceptors.remove(&block_hash) {
                    return self.announce(effect_builder, &accumulated_block);
                }
            }
            SignaturesFinality::NotSufficient => {
                return Effects::new();
            }
            SignaturesFinality::BogusValidators(ref public_keys) => {
                self.remove_signatures(&block_hash, public_keys)
            }
        }

        // TODO: Check for duplicated block and given height from given sender.

        // if let Some(other_block) = self
        //     .pending_blocks
        //     .entry(block.height())
        //     .or_default()
        //     .insert(sender, block.clone())
        // {
        //     if other_block == block {
        //         error!("received duplicated ReceivedBlock event");
        //         return Effects::new();
        //     }
        //     warn!(%sender, "peer sent different blocks at the same height; disconnecting");
        //     return effect_builder
        //         .announce_disconnect_from_peer(sender)
        //         .ignore();
        // }

        //self.execute_if_fully_signed(effect_builder, block.height())

        Effects::new()
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
            warn!(%error, "received invalid finality signature");
            return effect_builder
                .announce_disconnect_from_peer(sender)
                .ignore();
        }

        let block_hash = finality_signature.block_hash;
        let validator_weights = BTreeMap::new(); // TODO: Use proper weights.
        let mut has_sufficient_signatures = match self.block_acceptors.entry(block_hash) {
            Entry::Vacant(entry) => {
                match BlockAcceptor::new_from_finality_signature(finality_signature) {
                    Ok(block_acceptor) => entry.insert(block_acceptor),
                    Err(_error) => {
                        return effect_builder
                            .announce_disconnect_from_peer(sender)
                            .ignore()
                    }
                };
                SignaturesFinality::NotSufficient
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
                accumulated_block
                    .has_sufficient_signatures(self.fault_tolerance_fraction, &validator_weights)
            }
        };

        for _ in 0..2 {
            match has_sufficient_signatures {
                SignaturesFinality::Sufficient => {
                    if let Some(accumulated_block) = self.block_acceptors.remove(&block_hash) {
                        return self.announce(effect_builder, &accumulated_block);
                    }
                    error!(%block_hash, "expected to have block");
                    return Effects::new();
                }
                SignaturesFinality::NotSufficient => {
                    return Effects::new();
                }
                SignaturesFinality::BogusValidators(ref public_keys) => {
                    self.remove_signatures(&block_hash, public_keys);

                    has_sufficient_signatures = self
                        .block_acceptors
                        .get(&block_hash)
                        .map(|accumulated_block| {
                            accumulated_block.has_sufficient_signatures(
                                self.fault_tolerance_fraction,
                                &BTreeMap::new(),
                            )
                        })
                        .unwrap_or_else(|| {
                            error!(%block_hash, "should have block");
                            SignaturesFinality::BogusValidators(public_keys.clone())
                        })
                }
            }
        }

        error!(%block_hash, ?has_sufficient_signatures, "should have removed bogus validators");

        // TODO: Special care for a switch block?

        // - proceed to execute
        // - store block
        // - remove from block_acceptors
        //}

        // TODO: Reject if the era is out of the range we currently handle.
        // TODO: Limit the number of items per peer.

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

    fn remove_signatures(&mut self, block_hash: &BlockHash, signers: &[PublicKey]) {
        if let Some(accumulated_block) = self.block_acceptors.get_mut(block_hash) {
            accumulated_block.remove_signatures(signers);
        }
    }

    fn highest_known_block(&self) -> Option<(BlockHash, u64)> {
        let mut ret: Option<(BlockHash, u64)> = None;
        for (next_key, next_value) in self
            .block_acceptors
            .iter()
            .filter(|(next_key, next_value)| next_value.has_block_added())
        {
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

    fn highest_executable_block(&self) -> Option<(BlockHash, u64)> {
        let era_validator_weights = BTreeMap::new(); //something akin to era_validator_weights_for_block()
        let mut ret: Option<(BlockHash, u64)> = None;
        for (next_key, next_value) in
            self.block_acceptors
                .iter()
                .filter(|(next_key, next_value)| {
                    next_value.can_execute(self.fault_tolerance_fraction, &era_validator_weights)
                })
        {
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
        }
    }
}
