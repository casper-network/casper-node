use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use casper_types::ExecutionResult;
use derive_more::From;

use crate::{
    effect::requests::LinearChainRequest,
    types::{Block, BlockSignatures, DeployHash, FinalitySignature},
};

#[derive(Debug, From)]
pub enum Event<I> {
    /// A linear chain request issued by another node in the network.
    #[from]
    Request(LinearChainRequest<I>),
    /// New linear chain block has been produced.
    NewLinearChainBlock {
        /// The block.
        block: Box<Block>,
        /// The deploys' execution results.
        execution_results: HashMap<DeployHash, ExecutionResult>,
    },
    /// Linear chain block we already know but we may refinalize it when syncing protocol state.
    KnownLinearChainBlock(Box<Block>),
    /// Finality signature received.
    /// Not necessarily _new_ finality signature.
    FinalitySignatureReceived(Box<FinalitySignature>, bool),
    /// The result of putting a block to storage.
    PutBlockResult {
        /// The block.
        block: Box<Block>,
    },
    /// The result of requesting finality signatures from storage to add pending signatures.
    GetStoredFinalitySignaturesResult(Box<FinalitySignature>, Option<Box<BlockSignatures>>),
    /// Result of testing if creator of the finality signature is bonded validator.
    IsBonded(Option<Box<BlockSignatures>>, Box<FinalitySignature>, bool),
}

impl<I: Display> Display for Event<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(req) => write!(f, "linear chain request: {}", req),
            Event::NewLinearChainBlock { block, .. } => {
                write!(f, "linear chain new block: {}", block.hash())
            }
            Event::KnownLinearChainBlock(block) => {
                write!(f, "linear chain known block: {}", block.hash())
            }
            Event::FinalitySignatureReceived(fs, gossiped) => write!(
                f,
                "linear-chain new finality signature for block: {}, from: {}, external: {}",
                fs.block_hash, fs.public_key, gossiped
            ),
            Event::PutBlockResult { .. } => write!(f, "linear-chain put-block result"),
            Event::GetStoredFinalitySignaturesResult(finality_signature, maybe_signatures) => {
                write!(
                    f,
                    "linear chain get-stored-finality-signatures result for {} found: {}",
                    finality_signature.block_hash,
                    maybe_signatures.is_some(),
                )
            }
            Event::IsBonded(_block, fs, is_bonded) => {
                write!(
                    f,
                    "linear chain is-bonded for era {} validator {}, is_bonded: {}",
                    fs.era_id, fs.public_key, is_bonded
                )
            }
        }
    }
}
