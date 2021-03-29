use thiserror::Error;

use casper_execution_engine::{core::engine_state, shared::newtypes::Blake2bHash};

use crate::types::BlockHash;

#[derive(Error, Debug)]
pub enum FastSyncError {
    #[error(transparent)]
    ExecutionEngineError(#[from] engine_state::Error),

    #[error(transparent)]
    PrometheusError(#[from] prometheus::Error),

    #[error("Could not fetch trie key: {trie_key}")]
    RanOutOfFetchTrieRetries { trie_key: Blake2bHash },

    #[error("Could not fetch block hash: {block_hash}")]
    RanOutOfHeaderByHashFetchRetries { block_hash: BlockHash },

    #[error("Could not fetch block by height: {height}")]
    RanOutOfHeaderByHeightFetchRetries { height: u64 },
}
