use std::fmt::Debug;

use thiserror::Error;

use casper_execution_engine::{
    core::engine_state, shared::stored_value::StoredValue, storage::trie::Trie,
};
use casper_types::{EraId, Key, ProtocolVersion};

use crate::{
    components::fetcher::FetcherError,
    types::{BlockHeader, BlockHeaderWithMetadata},
};

#[derive(Error, Debug)]
pub enum LinearChainSyncError<I>
where
    I: Eq + Debug + 'static,
{
    #[error(transparent)]
    ExecutionEngineError(#[from] engine_state::Error),

    #[error(
        "Cannot get trusted validators for such an early era. \
         trusted header: {trusted_header:?}, \
         last emergency restart era id: {maybe_last_emergency_restart_era_id:?}"
    )]
    TrustedHeaderEraTooEarly {
        trusted_header: Box<BlockHeader>,
        maybe_last_emergency_restart_era_id: Option<EraId>,
    },

    #[error(
        "Current version is {current_version}, but retrieved block header with future version: \
         {block_header_with_future_version:?}"
    )]
    RetrievedBlockHeaderFromFutureVersion {
        current_version: ProtocolVersion,
        block_header_with_future_version: Box<BlockHeader>,
    },

    #[error(transparent)]
    BlockHeaderFetcherError(#[from] FetcherError<BlockHeader, I>),

    #[error(transparent)]
    BlockHeaderWithMetadataFetcherError(#[from] FetcherError<BlockHeaderWithMetadata, I>),

    #[error(transparent)]
    TrieFetcherError(#[from] FetcherError<Trie<Key, StoredValue>, I>),

    #[error("Could not store block header: {block_header}")]
    CouldNotStoreBlockHeader { block_header: Box<BlockHeader> },
}
