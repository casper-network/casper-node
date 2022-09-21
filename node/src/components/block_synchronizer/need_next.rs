use datasize::DataSize;

use crate::types::{BlockHash, DeployHash};
use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

#[derive(DataSize, Debug)]
pub(crate) enum NeedNext {
    Nothing,
    BlockHeader(BlockHash),
    BlockBody(BlockHash),
    FinalitySignatures(BlockHash, EraId, Vec<PublicKey>),
    GlobalState(Digest),
    Deploy(DeployHash),
    ExecutionResults(DeployHash),
    EraValidators(EraId),
    Peers,
}
