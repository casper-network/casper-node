use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
    marker::PhantomData,
};

use derive_more::From;
use futures::FutureExt;
use rand::{CryptoRng, Rng};
use tracing::{debug, error, info, warn};

use super::{storage::Storage, Component};
use crate::{
    components::storage::Value,
    crypto::asymmetric_key::Signature,
    effect::{
        requests::{ConsensusRequest, LinearChainRequest, NetworkRequest, StorageRequest},
        EffectExt, Effects,
    },
    protocol::Message,
    types::{Block, BlockHash, DeployHash, ExecutionResult},
};

#[derive(Debug, From)]
pub enum Event<I> {
    /// A linear chain request issued by another node in the network.
    #[from]
    Request(LinearChainRequest<I>),
    /// New linear chain block has been produced.
    LinearChainBlock {
        /// The block.
        block: Block,
        /// The deploys' execution results.
        execution_results: HashMap<DeployHash, ExecutionResult>,
    },
    /// A continuation for `GetBlock` scenario.
    GetBlockResult(BlockHash, Option<Block>, I),
    /// New finality signature.
    NewFinalitySignature(BlockHash, Signature),
    /// The result of putting a block to storage.
    PutBlockResult {
        /// The block.
        block: Block,
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
        }
    }
}

#[derive(Debug)]
pub(crate) struct LinearChain<I> {
    /// A temporary workaround.
    linear_chain: Vec<Block>,
    /// The last block this component put to storage which is presumably the last block in the
    /// linear chain.
    last_block: Option<Block>,
    _marker: PhantomData<I>,
}

impl<I> LinearChain<I> {
    pub fn new() -> Self {
        LinearChain {
            linear_chain: Vec::new(),
            last_block: None,
            _marker: PhantomData,
        }
    }

    pub fn linear_chain(&self) -> &Vec<Block> {
        &self.linear_chain
    }
}

impl<I, REv, R> Component<REv, R> for LinearChain<I>
where
    REv: From<StorageRequest<Storage>>
        + From<ConsensusRequest>
        + From<NetworkRequest<I, Message>>
        + Send,
    R: Rng + CryptoRng + ?Sized,
    I: Display + Send + 'static,
{
    type Event = Event<I>;

    fn handle_event(
        &mut self,
        effect_builder: crate::effect::EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(LinearChainRequest::BlockRequest(block_hash, sender)) => effect_builder
                .get_block_from_storage(block_hash)
                .event(move |maybe_block| Event::GetBlockResult(block_hash, maybe_block, sender)),
            Event::Request(LinearChainRequest::LastFinalizedBlock(responder)) => {
                responder.respond(self.last_block.clone()).ignore()
            }
            Event::GetBlockResult(block_hash, maybe_block, sender) => {
                match maybe_block {
                    None => {
                        debug!("failed to get {} for {}", block_hash, sender);
                        Effects::new()
                    },
                    Some(block) => match Message::new_get_response(&block) {
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
                .put_block_to_storage(Box::new(block.clone()))
                .event(move |_| Event::PutBlockResult{ block, execution_results })
            },
            Event::PutBlockResult { block, execution_results } => {
                self.linear_chain.push(block.clone());
                self.last_block = Some(block.clone());

                let block_header = block.take_header();
                let block_hash = block_header.hash();
                let era_id = block_header.era_id();
                let height = block_header.height();

                // Using `Debug` impl for the `block_hash` to not truncate it.
                info!(?block_hash, ?era_id, ?height, "Linear chain block stored.");

                let mut effects = effect_builder.put_execution_results_to_storage(block_hash, execution_results).ignore();
                effects.extend(
                    effect_builder.handle_linear_chain_block(block_header)
                    .event(move |signature| Event::NewFinalitySignature(block_hash, signature)));
                effects
            },
            Event::NewFinalitySignature(block_hash, signature) => {
                effect_builder
                .clone()
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
