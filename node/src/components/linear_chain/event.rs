use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use casper_types::ExecutionResult;

use crate::{
    effect::incoming::FinalitySignatureIncoming,
    types::{ActivationPoint, Block, BlockSignatures, DeployHash, FinalitySignature},
};

#[derive(Debug)]
pub(crate) enum Event {
    /// New linear chain block has been produced.
    NewLinearChainBlock {
        /// The block.
        block: Box<Block>,
        /// The deploys' execution results.
        execution_results: HashMap<DeployHash, ExecutionResult>,
    },
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
    /// We stored the last block before the next upgrade, with a complete set of signatures.
    Upgrade,
    /// Got the result of checking for an upgrade activation point.
    GotUpgradeActivationPoint(ActivationPoint),
}

impl From<FinalitySignatureIncoming> for Event {
    fn from(incoming: FinalitySignatureIncoming) -> Self {
        Event::FinalitySignatureReceived(incoming.message, true)
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::NewLinearChainBlock { block, .. } => {
                write!(f, "linear chain new block: {}", block.hash())
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
            Event::Upgrade => write!(f, "linear chain: shut down for upgrade"),
            Event::GotUpgradeActivationPoint(activation_point) => write!(
                f,
                "linear chain got upgrade activation point {}",
                activation_point
            ),
        }
    }
}
