mod error;
mod operations;

use std::fmt::Debug;

use datasize::DataSize;

use crate::types::BlockHeader;

pub(crate) use error::LinearChainSyncError;
pub(crate) use operations::{run_fast_sync_task, KeyBlockInfo};

#[derive(DataSize, Debug)]
/// Both NotGoingToSync and Done pass onwards the header of the switch block created during those
/// states.
pub(crate) enum LinearChainSyncState {
    NotGoingToSync {
        switch_block_header: Box<BlockHeader>,
    },
    Done {
        switch_block_header: Box<BlockHeader>,
    },
    Syncing,
}

impl LinearChainSyncState {
    /// Returns `true` if we have finished syncing linear chain.
    pub fn is_synced(&self) -> bool {
        !matches!(self, Self::Syncing)
    }
}
