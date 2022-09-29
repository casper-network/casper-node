use datasize::DataSize;

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use crate::types::{BlockHash, DeployHash};
use super::execution_results_acquisition::Need;

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
    ExecutionResults(Need),
    EraValidators(EraId),
    Peers(BlockHash),
}
