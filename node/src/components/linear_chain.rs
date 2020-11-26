use std::{
    collections::HashMap,
    convert::Infallible,
    fmt::{self, Display, Formatter},
    marker::PhantomData,
};

use datasize::DataSize;
use derive_more::From;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use super::Component;
use crate::{
    crypto::{
        self,
        asymmetric_key::{PublicKey, Signature},
    },
    effect::{
        announcements::LinearChainAnnouncement,
        requests::{ConsensusRequest, LinearChainRequest, NetworkRequest, StorageRequest},
        EffectExt, Effects, Responder,
    },
    protocol::Message,
    types::{json_compatibility::ExecutionResult, Block, BlockByHeight, BlockHash, DeployHash},
    NodeRng,
};

/// Finality signature that can be gossiped between nodes or sent to clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalitySignature {
    block_hash: BlockHash,
    signature: Signature,
    public_key: PublicKey,
}

impl FinalitySignature {
    pub fn new(block_hash: BlockHash, signature: Signature, public_key: PublicKey) -> Self {
        FinalitySignature {
            block_hash,
            signature,
            public_key,
        }
    }

    pub fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Verifies whether the signature is correct.
    pub fn verify(&self) -> crypto::asymmetric_key::Result<()> {
        crypto::asymmetric_key::verify(self.block_hash.inner(), &self.signature, &self.public_key)
    }
}

impl Display for FinalitySignature {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "finality signature for block hash {}, from {}",
            &self.block_hash, &self.public_key
        )
    }
}

impl<I> From<FinalitySignature> for Event<I> {
    fn from(fs: FinalitySignature) -> Self {
        Event::NewFinalitySignature(Box::new(fs))
    }
}

impl<I> From<Box<FinalitySignature>> for Event<I> {
    fn from(fs: Box<FinalitySignature>) -> Self {
        Event::NewFinalitySignature(fs)
    }
}

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
    NewFinalitySignature(Box<FinalitySignature>),
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
            Event::NewFinalitySignature(fs) => write!(
                f,
                "linear-chain new finality signature for block: {}, from: {}",
                fs.block_hash(),
                fs.public_key(),
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
    REv: From<StorageRequest>
        + From<ConsensusRequest>
        + From<NetworkRequest<I, Message>>
        + From<LinearChainAnnouncement>
        + Send,
    I: Display + Send + 'static,
{
    type Event = Event<I>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: crate::effect::EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(LinearChainRequest::BlockRequest(block_hash, sender)) => effect_builder
                .get_block_from_storage(block_hash)
                .event(move |maybe_block| {
                    Event::GetBlockResult(block_hash, maybe_block.map(Box::new), sender)
                }),
            Event::Request(LinearChainRequest::BlockAtHeightLocal(height, responder)) => {
                effect_builder
                    .get_block_at_height(height)
                    .event(move |block| {
                        Event::GetBlockByHeightResultLocal(height, block.map(Box::new), responder)
                    })
            }
            Event::Request(LinearChainRequest::BlockAtHeight(height, sender)) => effect_builder
                .get_block_at_height(height)
                .event(move |maybe_block| {
                    Event::GetBlockByHeightResult(height, maybe_block.map(Box::new), sender)
                }),
            Event::GetBlockByHeightResultLocal(_height, block, responder) => {
                responder.respond(block.map(|boxed| *boxed)).ignore()
            }
            Event::GetBlockByHeightResult(block_height, maybe_block, sender) => {
                let block_at_height = match maybe_block {
                    None => {
                        debug!("failed to get {} for {}", block_height, sender);
                        BlockByHeight::Absent(block_height)
                    }
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
            Event::GetBlockResult(block_hash, maybe_block, sender) => match maybe_block {
                None => {
                    debug!("failed to get {} for {}", block_hash, sender);
                    Effects::new()
                }
                Some(block) => match Message::new_get_response(&*block) {
                    Ok(message) => effect_builder.send_message(sender, message).ignore(),
                    Err(error) => {
                        error!("failed to create get-response {}", error);
                        Effects::new()
                    }
                },
            },
            Event::LinearChainBlock {
                block,
                execution_results,
            } => effect_builder
                .put_block_to_storage(block.clone())
                .event(move |_| Event::PutBlockResult {
                    block,
                    execution_results,
                }),
            Event::PutBlockResult {
                block,
                execution_results,
            } => {
                // TODO: Remove once we can return all linear chain blocks from persistent storage.
                self.linear_chain.push(*block.clone());

                let block_header = block.take_header();
                let block_hash = block_header.hash();
                let era_id = block_header.era_id();
                let height = block_header.height();
                info!(?block_hash, ?era_id, ?height, "Linear chain block stored.");
                let mut effects = effect_builder
                    .put_execution_results_to_storage(block_hash, execution_results)
                    .ignore();
                effects.extend(
                    effect_builder
                        .handle_linear_chain_block(block_header.clone())
                        .event(move |fs| Event::NewFinalitySignature(Box::new(fs))),
                );
                effects.extend(
                    effect_builder
                        .announce_block_added(block_hash, block_header)
                        .ignore(),
                );
                effects
            }
            Event::NewFinalitySignature(fs) => {
                let FinalitySignature {
                    block_hash,
                    public_key,
                    ..
                } = *fs;
                if let Err(err) = fs.verify() {
                    error!(%block_hash, %public_key, %err, "received invalid finality signature");
                    return Effects::new();
                }
                debug!(%block_hash, %public_key, "received new finality signature");
                async move {
                    let maybe_block = effect_builder
                        .get_block_from_storage(block_hash)
                        .await;
                    match maybe_block {
                        None => {
                            let block_hash = fs.block_hash();
                            let public_key = fs.public_key();
                            warn!(%block_hash, %public_key, "received a signature for a block that was not found in storage");
                        }
                        Some(mut block) => {
                            if !block.contains_proof(fs.signature()) {
                                block.append_proof(fs.signature);
                                let _ = effect_builder
                                    .put_block_to_storage(Box::new(block))
                                    .await;
                                let _ = effect_builder
                                    .broadcast_message(Message::FinalitySignature(fs))
                                    .await;
                            }
                        }
                    }
                }.ignore()
            }
        }
    }
}
