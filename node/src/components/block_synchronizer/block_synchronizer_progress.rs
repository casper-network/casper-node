use std::fmt::{Display, Formatter};

use casper_types::{BlockHash, EraId, Timestamp};

#[derive(Debug)]
pub(crate) enum BlockSynchronizerProgress {
    Idle,
    Syncing(BlockHash, Option<u64>, Timestamp),
    Executing(BlockHash, u64, EraId),
    Synced(BlockHash, u64, EraId),
}

impl BlockSynchronizerProgress {
    pub(crate) fn is_active(&self) -> bool {
        match self {
            BlockSynchronizerProgress::Idle | BlockSynchronizerProgress::Synced(_, _, _) => false,
            BlockSynchronizerProgress::Syncing(_, _, _)
            | BlockSynchronizerProgress::Executing(_, _, _) => true,
        }
    }
}

impl Display for BlockSynchronizerProgress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let display_height = |f: &mut Formatter<'_>, maybe_height: &Option<u64>| match maybe_height
        {
            Some(height) => write!(f, "block {height}"),
            None => write!(f, "unknown block height"),
        };
        match self {
            BlockSynchronizerProgress::Idle => write!(f, "block synchronizer idle"),
            BlockSynchronizerProgress::Syncing(block_hash, block_height, timestamp) => {
                write!(f, "block synchronizer syncing ")?;
                display_height(f, block_height)?;
                write!(f, "{}, {}", timestamp, block_hash)
            }
            BlockSynchronizerProgress::Executing(block_hash, block_height, era_id) => {
                write!(
                    f,
                    "block synchronizer executing block {}, {}, {}",
                    block_height, block_hash, era_id
                )
            }
            BlockSynchronizerProgress::Synced(block_hash, block_height, era_id) => {
                write!(
                    f,
                    "block synchronizer synced block {}, {}, {}",
                    block_height, block_hash, era_id
                )
            }
        }
    }
}
