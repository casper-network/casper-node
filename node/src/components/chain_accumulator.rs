use std::{
    collections::{BTreeMap, BTreeSet},
    convert::Infallible,
};

use casper_types::EraId;
use datasize::DataSize;
use tracing::{error, warn};

use crate::{
    components::Component,
    effect::{Effect, EffectBuilder, Effects},
    types::{Block, BlockHash, FinalitySignature, NodeId},
    NodeRng,
};

/// A cache of pending blocks and finality signatures that executes and stores fully signed blocks.
///
/// Caches incoming blocks and finality signatures. Invokes execution and then storage of a block
/// once it has a quorum of finality signatures. At that point it also starts gossiping that block
/// onwards.
#[derive(DataSize, Debug)]
pub(crate) struct ChainAccumulator {
    pending_blocks: BTreeMap<u64, BTreeMap<NodeId, Block>>,
    pending_signatures: BTreeMap<EraId, BTreeMap<BlockHash, BTreeSet<(NodeId, FinalitySignature)>>>,
}

impl ChainAccumulator {
    fn handle_block<REv>(
        effect_builder: EffectBuilder<REv>,
        block: Block,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: Send,
    {
        if let Some((faulty_sender, other_block)) = self
            .pending_blocks
            .entry_mut(block.height())
            .or_default()
            .insert(sender, block.clone())
        {
            if other_block == block {
                error!("received duplicated ReceivedBlock event");
                return Effects::new();
            }
            warn!(%faulty_sender, "peer sent multiple blocks at the same height; disconnecting");
            return effect_builder.announce_disconnect_from_peer(faulty_sender);
        }
        self.execute_if_fully_signed(effect_builder, block.height())
    }

    fn handle_finality_signature<REv>(
        effect_builder: EffectBuilder<REv>,
        finality_signature: FinalitySignature,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: Send,
    {
        // TODO: Reject if the era is out of the range we currently handle.
        self.pending_signatures
            .entry_mut(finality_signature.era_id)
            .or_default()
            .entry_mut(finality_signature.block_hash)
            .or_default()
            .insert((sender, finality_signature));
        todo!()
    }

    fn execute_if_fully_signed<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        height: u64,
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

impl<REv> Component<REv> for ChainAccumulator
where
    REv: Send,
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
