use super::{consensus::EraId, storage::Storage, Component};
use crate::components::storage::Value;
use crate::{
    crypto::asymmetric_key::Signature,
    effect::{self, EffectExt, Effects},
    effect::{
        requests::{LinearChainRequest, StorageRequest},
        Responder,
    },
    types::{Block, BlockHash, BlockHeader},
};
use derive_more::From;
use effect::requests::ConsensusRequest;
use futures::FutureExt;
use rand::{CryptoRng, Rng};
use std::fmt::Display;
use tracing::warn;

#[derive(Debug, From)]
pub enum Event {
    /// A linear chain request issued by another node in the network.
    #[from]
    Request(LinearChainRequest),
    /// New linear chain block has been produced.
    LinearChainBlock(Block),
    /// A continuation for `GetHeader` scenario.
    GetHeaderResult(Option<BlockHeader>, Responder<Option<BlockHeader>>),
    /// A continuation for `GetBlock` scenario.
    GetBlockResult(Option<Block>, Responder<Option<Block>>),
    /// New finality signature.
    NewFinalitySignature(BlockHash, Signature),
    PutBlockResult(EraId, BlockHash),
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Request(req) => write!(f, "linear-chain request: {}", req),
            Event::LinearChainBlock(b) => write!(f, "linear-chain new block: {}", b.hash()),
            Event::GetHeaderResult(_, _) => write!(f, "linear-chain get-header result"),
            Event::GetBlockResult(_, _) => write!(f, "linear-chain get-block result"),
            Event::NewFinalitySignature(bh, _) => {
                write!(f, "linear-chain new finality signature for block: {}", bh)
            }
            Event::PutBlockResult(_, _) => write!(f, "linear-chain put-block result"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct LinearChain;

impl LinearChain {
    pub fn new() -> Self {
        LinearChain
    }
}

impl<REv, R> Component<REv, R> for LinearChain
where
    REv: From<StorageRequest<Storage>> + From<ConsensusRequest> + Send,
    R: Rng + CryptoRng + ?Sized,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: crate::effect::EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(LinearChainRequest::BlockHeaderRequest(bh, responder)) => effect_builder
                .get_block_header_from_storage(bh)
                .event(move |maybe_header| Event::GetHeaderResult(maybe_header, responder)),
            Event::Request(LinearChainRequest::BlockRequest(bh, responder)) => effect_builder
                .get_block_from_storage(bh)
                .event(move |maybe_block| Event::GetBlockResult(maybe_block, responder)),
            Event::GetHeaderResult(maybe_header, responder) => {
                responder.respond(maybe_header).ignore()
            }
            Event::GetBlockResult(maybe_block, responder) => {
                responder.respond(maybe_block).ignore()
            }
            Event::LinearChainBlock(block) => {
                let era_id = block.header().era_id();
                let block_hash = *block.hash();
                effect_builder
                .put_block_to_storage(Box::new(block))
                .event(move |_| Event::PutBlockResult(era_id, block_hash))
            },
            Event::PutBlockResult(era_id, block_hash) =>
                effect_builder.sign_linear_chain_block(era_id, block_hash)
                .event(move |signature| Event::NewFinalitySignature(block_hash, signature)),
            Event::NewFinalitySignature(bh, signature) => {
                effect_builder
                .clone()
                    .get_block_from_storage(bh)
                    .then(move |maybe_block| match maybe_block {
                        Some(mut block) => {
                            block.append_proof(signature);
                            effect_builder.put_block_to_storage(Box::new(block))
                        }
                        None => {
                            warn!("Received a signature for {} but block was not found in the Linear chain storage", bh);
                            panic!("Unhandled")
                        }
                    })
                    .ignore()
            }
        }
    }
}
