use std::{
    collections::HashMap,
    convert::Infallible,
    fmt::{self, Display, Formatter},
    marker::PhantomData,
};

use datasize::DataSize;
use derive_more::From;
use itertools::Itertools;
use tracing::{debug, error, info, warn};

use casper_types::ExecutionResult;

use super::Component;
use crate::{
    crypto::asymmetric_key::PublicKey,
    effect::{
        announcements::LinearChainAnnouncement,
        requests::{ConsensusRequest, LinearChainRequest, NetworkRequest, StorageRequest},
        EffectExt, EffectOptionExt, Effects, Responder,
    },
    protocol::Message,
    types::{Block, BlockByHeight, BlockHash, DeployHash, FinalitySignature},
    NodeRng,
};

/// The maximum number of finality signatures from a single validator we keep in memory while
/// waiting for their block.
const MAX_PENDING_FINALITY_SIGNATURES_PER_VALIDATOR: usize = 1000;

impl<I> From<Box<FinalitySignature>> for Event<I> {
    fn from(fs: Box<FinalitySignature>) -> Self {
        Event::FinalitySignatureReceived(fs)
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
    /// Finality signature received.
    /// Not necessarily _new_ finality signature.
    FinalitySignatureReceived(Box<FinalitySignature>),
    /// The result of putting a block to storage.
    PutBlockResult {
        /// The block.
        block: Box<Block>,
        /// The deploys' execution results.
        execution_results: HashMap<DeployHash, ExecutionResult>,
    },
    /// The result of requresting a block from storage to add a finality signature to it.
    GetBlockForFinalitySignaturesResult(BlockHash, Option<Box<Block>>),
}

impl<I: Display> Display for Event<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(req) => write!(f, "linear chain request: {}", req),
            Event::LinearChainBlock { block, .. } => {
                write!(f, "linear chain new block: {}", block.hash())
            }
            Event::GetBlockResult(block_hash, maybe_block, peer) => write!(
                f,
                "linear chain get-block for {} from {} found: {}",
                block_hash,
                peer,
                maybe_block.is_some()
            ),
            Event::FinalitySignatureReceived(fs) => write!(
                f,
                "linear-chain new finality signature for block: {}, from: {}",
                fs.block_hash, fs.public_key,
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
            Event::GetBlockForFinalitySignaturesResult(block_hash, maybe_block) => {
                write!(
                    f,
                    "linear chain get-block-for-finality-signatures-result for {} found: {}",
                    block_hash,
                    maybe_block.is_some()
                )
            }
        }
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct LinearChain<I> {
    /// A temporary workaround.
    // TODO: Refactor to proper LRU cache.
    linear_chain: Vec<Block>,
    /// Finality signatures to be inserted in a block once it is available.
    pending_finality_signatures: HashMap<PublicKey, HashMap<BlockHash, FinalitySignature>>,
    _marker: PhantomData<I>,
}

impl<I> LinearChain<I> {
    pub fn new() -> Self {
        LinearChain {
            linear_chain: Vec::new(),
            pending_finality_signatures: HashMap::new(),
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
                        .map_some(move |fs| Event::FinalitySignatureReceived(Box::new(fs))),
                );
                effects.extend(
                    effect_builder
                        .announce_block_added(block_hash, block_header)
                        .ignore(),
                );
                effects
            }
            Event::FinalitySignatureReceived(fs) => {
                let FinalitySignature {
                    block_hash,
                    public_key,
                    ..
                } = *fs;
                // TODO: Also verify that the public key belongs to a bonded validator!
                if let Err(err) = fs.verify() {
                    warn!(%block_hash, %public_key, %err, "received invalid finality signature");
                    return Effects::new();
                }
                debug!(%block_hash, %public_key, "received new finality signature");
                let sigs = self
                    .pending_finality_signatures
                    .entry(public_key)
                    .or_default();
                // Limit the memory we use for storing unknown signatures from each validator.
                if sigs.len() >= MAX_PENDING_FINALITY_SIGNATURES_PER_VALIDATOR {
                    warn!(
                        %block_hash, %public_key,
                        "received too many finality signatures for unknown blocks"
                    );
                    return Effects::new();
                }
                // Add the pending signature and request the block from storage.
                if sigs.insert(block_hash, *fs).is_some() {
                    return Effects::new(); // Signature was already known.
                }
                effect_builder
                    .get_block_from_storage(block_hash)
                    .event(move |maybe_block| {
                        let maybe_box_block = maybe_block.map(Box::new);
                        Event::GetBlockForFinalitySignaturesResult(block_hash, maybe_box_block)
                    })
            }
            Event::GetBlockForFinalitySignaturesResult(block_hash, None) => {
                let signers = self
                    .pending_finality_signatures
                    .values()
                    .filter_map(|sigs| sigs.get(&block_hash))
                    .map(|fs| format!("{}", fs.public_key))
                    .collect_vec();
                if !signers.is_empty() {
                    // We have signatures for an unknown block. Print log messages.
                    warn!(
                       %block_hash, signers = %signers.join(", "),
                       "received signatures for a block that was not found in storage"
                    )
                }
                Effects::new()
            }
            Event::GetBlockForFinalitySignaturesResult(block_hash, Some(mut block)) => {
                // Remove the signatures for the block from the pending ones.
                let sigs: Vec<_> = self
                    .pending_finality_signatures
                    .values_mut()
                    .filter_map(|sigs| sigs.remove(&block_hash).map(Box::new))
                    // TODO: Store signatures by public key.
                    .filter(|fs| !block.proofs().iter().any(|proof| proof == &fs.signature))
                    .collect();
                self.pending_finality_signatures
                    .retain(|_, sigs| !sigs.is_empty());
                let mut effects = Effects::new();
                if sigs.is_empty() {
                    return effects; // No new signatures to add.
                }
                // Add new signatures and send the updated block to storage.
                for fs in sigs {
                    block.append_proof(fs.signature);
                    let message = Message::FinalitySignature(fs.clone());
                    effects.extend(effect_builder.broadcast_message(message).ignore());
                    effects.extend(effect_builder.announce_finality_signature(fs).ignore());
                }
                effects.extend(effect_builder.put_block_to_storage(block).ignore());
                effects
            }
        }
    }
}
