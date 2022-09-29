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
    /// We want the merkle root hash stored in global state under the ChecksumRegistry key for the
    /// execution results.
    ExecutionResultsRootHash {
        global_state_root_hash: Digest,
    },
    ExecutionResults(BlockHash),
    EraValidators(EraId),
    Peers(BlockHash),
}
