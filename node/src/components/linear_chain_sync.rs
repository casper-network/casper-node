mod error;
mod operations;

use std::fmt::Debug;

use datasize::DataSize;

use crate::types::BlockHeader;

pub(crate) use error::LinearChainSyncError;
pub(crate) use operations::run_fast_sync_task;

#[derive(DataSize, Debug)]
pub(crate) enum LinearChainSyncState {
    NotGoingToSync,
    Syncing,
    Done(Box<BlockHeader>),
}

impl LinearChainSyncState {
    /// Returns `true` if we have finished syncing linear chain.
    pub fn is_synced(&self) -> bool {
        !matches!(self, Self::Syncing)
    }

    pub fn into_maybe_latest_block_header(self) -> Option<BlockHeader> {
        match self {
            Self::Done(latest_block_header) => Some(*latest_block_header),
            Self::NotGoingToSync | Self::Syncing => None,
        }
    }
}
