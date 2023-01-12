use std::fmt::{Display, Formatter};

use crate::types::BlockHash;
use casper_types::{EraId, Timestamp};

#[derive(Debug)]
pub(crate) enum BlockSynchronizerProgress {
    Idle,
    Syncing(BlockHash, Option<u64>, Timestamp),
    Executing(BlockHash, u64, EraId),
    Synced(BlockHash, u64, EraId),
}

impl Display for BlockSynchronizerProgress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockSynchronizerProgress::Idle => write!(f, "Idle",),
            BlockSynchronizerProgress::Syncing(block_hash, block_height, timestamp) => {
                write!(
                    f,
                    "block_height: {:?} timestamp: {} block_hash: {}",
                    block_height, timestamp, block_hash
                )
            }
            BlockSynchronizerProgress::Executing(block_hash, block_height, era_id) => {
                write!(
                    f,
                    "block_height: {} block_hash: {} era_id: {}",
                    block_height, block_hash, era_id
                )
            }
            BlockSynchronizerProgress::Synced(block_hash, block_height, era_id) => {
                write!(
                    f,
                    "block_height: {} block_hash: {} era_id: {}",
                    block_height, block_hash, era_id
                )
            }
        }
    }
}
