use datasize::DataSize;

use crate::types::{BlockHash, DeployHash};
use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

#[derive(DataSize, Debug, Clone)]
pub(crate) enum NeedNext {
    Nothing,
    BlockHeader(BlockHash),
    BlockBody(BlockHash),
    FinalitySignatures(BlockHash, EraId, Vec<PublicKey>),
    GlobalState(BlockHash, Digest),
    Deploy(BlockHash, DeployHash),
    ExecutionResults(BlockHash),
    EraValidators(EraId),
    Peers(BlockHash),
}
