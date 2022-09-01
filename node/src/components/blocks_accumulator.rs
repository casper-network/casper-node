use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    convert::Infallible,
};

use casper_types::{EraId, PublicKey};
use datasize::DataSize;
use num_rational::Ratio;
use tracing::{debug, error, warn};

use crate::{
    components::Component,
    effect::{announcements::BlocklistAnnouncement, Effect, EffectBuilder, EffectExt, Effects},
    types::{Block, BlockHash, BlockSignatures, FinalitySignature, NodeId},
    NodeRng,
};

use super::linear_chain::check_sufficient_block_signatures;

#[derive(DataSize, Debug)]
struct AccumulatedBlock {
    block_hash: BlockHash,
    block: Option<Block>,
    era_id: EraId,
    signatures: BTreeMap<PublicKey, FinalitySignature>,
    #[data_size(skip)]
    fault_tolerance_fraction: Ratio<u64>,
}

impl AccumulatedBlock {
    fn new_from_block_added(block: Block, fault_tolerance_fraction: Ratio<u64>) -> Self {
        Self {
            block_hash: *block.hash(),
            era_id: block.header().era_id(),
            block: Some(block),
            signatures: Default::default(),
            fault_tolerance_fraction,
        }
    }

    fn new_from_finality_signature(
        finality_signature: FinalitySignature,
        fault_tolerance_fraction: Ratio<u64>,
    ) -> Self {
        let mut signatures = BTreeMap::new();
        let era_id = finality_signature.era_id;
        let block_hash = finality_signature.block_hash;
        signatures.insert(finality_signature.public_key.clone(), finality_signature);
        Self {
            block_hash,
            block: None,
            era_id,
            signatures,
            fault_tolerance_fraction,
        }
    }

    fn register_signature(&mut self, finality_signature: FinalitySignature) {
        // TODO: What to do when we receive multiple valid finality_signature from single public_key?
        // TODO: What to do when we receive too many finality_signature from single peer?
        self.signatures
            .insert(finality_signature.public_key.clone(), finality_signature);
    }

    fn register_block(&mut self, block: Block) {
        if self.block.is_some() {
            warn!(block_hash = %block.hash(), "received duplicate block");
            return;
        }

        self.block = Some(block);
    }

    fn has_sufficient_signatures(&self) -> bool {
        let trusted_validator_weights = BTreeMap::new(); // TODO: Get proper weights here

        // TODO: Consider caching the sigs directly in the `BlockSignatures` struct, to avoid creating it
        // from `BTreeMap<PublicKey, FinalitySignature>` on every call.
        let mut block_signatures = BlockSignatures::new(self.block_hash, self.era_id);
        self.signatures
            .iter()
            .for_each(|(public_key, finality_signature)| {
                block_signatures.insert_proof(public_key.clone(), finality_signature.signature);
            });

        check_sufficient_block_signatures(
            &trusted_validator_weights,
            self.fault_tolerance_fraction,
            Some(&block_signatures),
        )
        .is_ok()
    }
}

/// A cache of pending blocks and finality signatures that executes and stores fully signed blocks.
///
/// Caches incoming blocks and finality signatures. Invokes execution and then storage of a block
/// once it has a quorum of finality signatures. At that point it also starts gossiping that block
/// onwards.
#[derive(DataSize, Debug)]
pub(crate) struct BlocksAccumulator {
    // pending_blocks: BTreeMap<u64, BTreeMap<NodeId, Block>>,
    // pending_signatures: BTreeMap<EraId, BTreeMap<BlockHash, BTreeSet<(NodeId, FinalitySignature)>>>,
    accumulated_blocks: BTreeMap<BlockHash, AccumulatedBlock>,
}

impl BlocksAccumulator {
    fn handle_block<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block: Block,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: Send + From<BlocklistAnnouncement>,
    {
        if let Err(err) = block.verify() {
            debug!(%err, "received invalid block");
            return Effects::new();
        }

        // To be read from chainspec
        let fault_tolerance_ratio = Ratio::new(1, 1);

        let block_hash = *block.hash();
        let has_sufficient_signatures = match self.accumulated_blocks.entry(block_hash) {
            Entry::Vacant(entry) => {
                entry.insert(AccumulatedBlock::new_from_block_added(
                    block,
                    fault_tolerance_ratio,
                ));
                false
            }
            Entry::Occupied(entry) => {
                let accumulated_block = entry.into_mut();
                accumulated_block.register_block(block);
                accumulated_block.has_sufficient_signatures()
            }
        };

        if has_sufficient_signatures {
            if let Some(accumulated_block) = self.accumulated_blocks.remove(&block_hash) {
                return self.execute(effect_builder, &accumulated_block);
            } else {
                error!(%block_hash, "expected to have block");
                return Effects::new();
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
        REv: Send,
    {
        if let Err(err) = finality_signature.verify() {
            debug!(%err, "received invalid finality signature");
            return Effects::new();
        }

        // To be read from chainspec
        let fault_tolerance_ratio: Ratio<u64> = Ratio::new(1, 1);

        let block_hash = finality_signature.block_hash;
        let has_sufficient_signatures = match self.accumulated_blocks.entry(block_hash) {
            Entry::Vacant(entry) => {
                entry.insert(AccumulatedBlock::new_from_finality_signature(
                    finality_signature,
                    fault_tolerance_ratio,
                ));
                false
            }
            Entry::Occupied(entry) => {
                let accumulated_block = entry.into_mut();
                accumulated_block.register_signature(finality_signature);
                accumulated_block.has_sufficient_signatures()
            }
        };

        if has_sufficient_signatures {
            if let Some(accumulated_block) = self.accumulated_blocks.remove(&block_hash) {
                return self.execute(effect_builder, &accumulated_block);
            } else {
                error!(%block_hash, "expected to have block");
                return Effects::new();
            }
        }

        //            BlocksAccumulator::execute(effect_builder, accumulated_block);

        // TODO: Special care for a switch block?

        // - proceed to execute
        // - store block
        // - remove from accumulated_blocks
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

    fn execute<REv>(
        &self,
        effect_builder: EffectBuilder<REv>,
        accumulated_block: &AccumulatedBlock,
    ) -> Effects<Event>
    where
        REv: Send,
    {
        todo!()
    }
}

#[derive(Debug)]
pub(crate) enum Event {
    ReceivedBlock {
        block: Block,
        sender: NodeId,
    },
    ReceivedFinalitySignature {
        finality_signature: FinalitySignature,
        sender: NodeId,
    },
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
                self.handle_block(effect_builder, block, sender)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => self.handle_finality_signature(effect_builder, finality_signature, sender),
        }
    }
}
