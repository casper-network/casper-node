use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
    marker::PhantomData,
};

use datasize::DataSize;
use derive_more::From;
use futures::FutureExt;
use tracing::{debug, error, info, warn};

use super::{storage::Storage, Component};
use crate::{
    crypto::asymmetric_key::Signature,
    effect::{
        announcements::LinearChainAnnouncement,
        requests::{ConsensusRequest, LinearChainRequest, NetworkRequest, StorageRequest},
        EffectExt, Effects, Responder,
    },
    protocol::Message,
    types::{
        json_compatibility::ExecutionResult, Block, BlockByHeight, BlockHash, CryptoRngCore,
        DeployHash,
    },
};

#[derive(Debug, From)]
pub enum Event<I> {
    /// A linear chain request issued by another node in the network.
    #[from]
    Request(LinearChainRequest<I>),
    /// New linear chain block has been produced.
    LinearChainBlock {
        /// The block.
        block: Box<Block>,
        /// The deploys' execution results.
        execution_results: HashMap<DeployHash, ExecutionResult>,
    },
    /// A continuation for `GetBlock` scenario.
    GetBlockResult(BlockHash, Option<Box<Block>>, I),
    /// A continuation for `BlockAtHeight` scenario.
    GetBlockByHeightResult(u64, Option<Box<Block>>, I),
    /// A continuation for `BlockAtHeightLocal` scenario.
    GetBlockByHeightResultLocal(u64, Option<Box<Block>>, Responder<Option<Block>>),
    /// New finality signature.
    NewFinalitySignature(BlockHash, Signature),
    /// The result of putting a block to storage.
    PutBlockResult {
        /// The block.
        block: Box<Block>,
        /// The deploys' execution results.
        execution_results: HashMap<DeployHash, ExecutionResult>,
    },
}

impl<I: Display> Display for Event<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(req) => write!(f, "linear-chain request: {}", req),
            Event::LinearChainBlock { block, .. } => {
                write!(f, "linear-chain new block: {}", block.hash())
            }
            Event::GetBlockResult(block_hash, maybe_block, peer) => write!(
                f,
                "linear-chain get-block for {} from {} found: {}",
                block_hash,
                peer,
                maybe_block.is_some()
            ),
            Event::NewFinalitySignature(block_hash, _) => write!(
                f,
                "linear-chain new finality signature for block: {}",
                block_hash
            ),
            Event::PutBlockResult { .. } => write!(f, "linear-chain put-block result"),
            Event::GetBlockByHeightResult(height, result, peer) => write!(
                f,
                "linear chain get-block-height for height {} from {} found: {}",
                height,
                peer,
                result.is_some()
            ),
            Event::GetBlockByHeightResultLocal(height, block, _) => write!(
                f,
                "linear chain get-block-height-local for height={} found={}",
                height,
                block.is_some()
            ),
        }
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct LinearChain<I> {
    /// A temporary workaround.
    // TODO: Refactor to proper LRU cache.
    linear_chain: Vec<Block>,
    _marker: PhantomData<I>,
}

impl<I> LinearChain<I> {
    pub fn new() -> Self {
        LinearChain {
            linear_chain: Vec::new(),
            _marker: PhantomData,
        }
    }

    // TODO: Remove once we can return all linear chain blocks from persistent storage.
    pub fn linear_chain(&self) -> &Vec<Block> {
        &self.linear_chain
    }
}

impl<I, REv> Component<REv> for LinearChain<I>
where
    REv: From<StorageRequest<Storage>>
        + From<ConsensusRequest>
        + From<NetworkRequest<I, Message>>
        + From<LinearChainAnnouncement>
        + Send,
    I: Display + Send + 'static,
{
    type Event = Event<I>;

    fn handle_event(
        &mut self,
        effect_builder: crate::effect::EffectBuilder<REv>,
        _rng: &mut dyn CryptoRngCore,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(LinearChainRequest::BlockRequest(block_hash, sender)) => effect_builder
                .get_block_from_storage(block_hash)
                .event(move |maybe_block| Event::GetBlockResult(block_hash, maybe_block.map(Box::new), sender)),
            Event::Request(LinearChainRequest::BlockAtHeightLocal(height, responder)) => {
                effect_builder
                    .get_block_at_height(height)
                    .event(move |block| Event::GetBlockByHeightResultLocal(height, block.map(Box::new), responder))
            }
            Event::Request(LinearChainRequest::BlockAtHeight(height, sender)) => {
                // Treat `linear_chain` as a cache of least-recently asked for blocks.
                // match self.linear_chain.get(height as usize).cloned() {
                //     Some(block) => effect_builder
                //         .immediately()
                //         .event(move |_| Event::GetBlockByHeightResult(height, Some(block), sender)),
                //     None =>
                effect_builder
                    .get_block_at_height(height)
                    .event(move |maybe_block| Event::GetBlockByHeightResult(height, maybe_block.map(Box::new), sender))
            }
            Event::GetBlockByHeightResultLocal(_height, block, responder) => {
                responder.respond(block.map(|boxed| *boxed)).ignore()
            }
            Event::GetBlockByHeightResult(block_height, maybe_block, sender) => {
                let block_at_height = match maybe_block {
                    None => {
                        debug!("failed to get {} for {}", block_height, sender);
                        BlockByHeight::Absent(block_height)
                    },
                    Some(block) => BlockByHeight::new(*block),
                };
                match Message::new_get_response(&block_at_height) {
                    Ok(message) => effect_builder.send_message(sender, message).ignore(),
                    Err(error) => {
                        error!("failed to create get-response {}", error);
                        Effects::new()
                    }
                }
            }
            Event::GetBlockResult(block_hash, maybe_block, sender) => {
                match maybe_block {
                    None => {
                        debug!("failed to get {} for {}", block_hash, sender);
                        Effects::new()
                    },
                    Some(block) => match Message::new_get_response(&*block) {
                        Ok(message) => effect_builder.send_message(sender, message).ignore(),
                        Err(error) => {
                            error!("failed to create get-response {}", error);
                            Effects::new()
                        }
                    }
                }
            }
            Event::LinearChainBlock{ block, execution_results } => {
                effect_builder
                .put_block_to_storage(block.clone())
                .event(move |_| Event::PutBlockResult{ block, execution_results })
            },
            Event::PutBlockResult { block, execution_results } => {
                // TODO: Remove once we can return all linear chain blocks from persistent storage.
                self.linear_chain.push(*block.clone());

                let block_header = block.take_header();
                let block_hash = block_header.hash();
                let era_id = block_header.era_id();
                let height = block_header.height();
                info!(?block_hash, ?era_id, ?height, "Linear chain block stored.");
                let mut effects = effect_builder.put_execution_results_to_storage(block_hash, execution_results).ignore();
                effects.extend(
                    effect_builder.handle_linear_chain_block(block_header.clone())
                    .event(move |signature| Event::NewFinalitySignature(block_hash, signature)));
                effects.extend(effect_builder.announce_block_added(block_hash, block_header).ignore());
                effects
            },
            Event::NewFinalitySignature(block_hash, signature) => {
                effect_builder
                    .get_block_from_storage(block_hash)
                    .then(move |maybe_block| match maybe_block {
                        Some(mut block) => {
                            block.append_proof(signature);
                            effect_builder.put_block_to_storage(Box::new(block))
                        }
                        None => {
                            warn!("Received a signature for {} but block was not found in the Linear chain storage", block_hash);
                            panic!("Unhandled")
                        }
                    })
                    .ignore()
            },
        }
    }
}
