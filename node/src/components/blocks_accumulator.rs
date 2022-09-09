mod block_acceptor;
mod error;
mod event;

use std::{
    collections::{btree_map::Entry, BTreeMap},
    convert::Infallible,
};

use datasize::DataSize;
use num_rational::Ratio;
use tracing::{error, warn};

use casper_types::PublicKey;

use crate::{
    components::Component,
    effect::{announcements::BlocklistAnnouncement, EffectBuilder, EffectExt, Effects},
    types::{BlockAdded, BlockHash, FetcherItem, FinalitySignature, NodeId},
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
    #[data_size(skip)]
    fault_tolerance_fraction: Ratio<u64>,
}

impl BlocksAccumulator {
    pub(crate) fn new(fault_tolerance_fraction: Ratio<u64>) -> Self {
        Self {
            block_acceptors: Default::default(),
            fault_tolerance_fraction,
        }
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
                accumulated_block
                    .has_sufficient_signatures(self.fault_tolerance_fraction, BTreeMap::new())
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
                accumulated_block.register_signature(finality_signature);
                accumulated_block
                    .has_sufficient_signatures(self.fault_tolerance_fraction, validator_weights)
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
                                BTreeMap::new(),
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
        if let Some(accumulated_block) = self.block_acceptors.get_mut(&block_hash) {
            accumulated_block.remove_signatures(signers);
        }
    }
}

impl<REv> Component<REv> for BlocksAccumulator
where
    REv: Send + From<BlocklistAnnouncement>,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::ReceivedBlock { block, sender } => {
                self.handle_block_added(effect_builder, block, sender)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => self.handle_finality_signature(effect_builder, finality_signature, sender),
        }
    }
}
