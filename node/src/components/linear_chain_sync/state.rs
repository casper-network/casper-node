use datasize::DataSize;

use crate::types::{BlockHash, BlockHeader};
use std::fmt::Display;

#[derive(DataSize, Debug)]
pub(crate) enum State {
    /// No syncing of the linear chain configured.
    None,
    /// Synchronizing the linear chain up until trusted hash.
    SyncingTrustedHash {
        /// Linear chain block to start sync from.
        trusted_hash: BlockHash,
        /// During synchronization we might see new eras being created.
        /// Track the highest height and wait until it's handled by consensus.
        highest_block_seen: u64,
        /// Chain of downloaded blocks from the linear chain.
        /// We will `pop()` when executing blocks.
        linear_chain: Vec<BlockHeader>,
        /// The most recent block we started to execute. This is updated whenever we start
        /// downloading deploys for the next block to be executed.
        latest_block: Box<Option<BlockHeader>>,
    },
    /// Synchronizing the descendants of the trusted hash.
    SyncingDescendants {
        /// The most recent block we started to execute. This is updated whenever we start
        /// downloading deploys for the next block to be executed.
        latest_block: Box<BlockHeader>,
        /// During synchronization we might see new eras being created.
        /// Track the highest height and wait until it's handled by consensus.
        highest_block_seen: u64,
    },
    /// Synchronizing done.
    Done,
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            State::None => write!(f, "None"),
            State::SyncingTrustedHash { trusted_hash, .. } => {
                write!(f, "SyncingTrustedHash(trusted_hash: {:?})", trusted_hash)
            }
            State::SyncingDescendants {
                highest_block_seen, ..
            } => write!(
                f,
                "SyncingDescendants(highest_block_seen: {})",
                highest_block_seen
            ),
            State::Done => write!(f, "Done"),
        }
    }
}

impl State {
    pub(crate) fn sync_trusted_hash(trusted_hash: BlockHash) -> Self {
        State::SyncingTrustedHash {
            trusted_hash,
            highest_block_seen: 0,
            linear_chain: Vec::new(),
            latest_block: Box::new(None),
        }
    }

    pub(crate) fn sync_descendants(latest_block: BlockHeader) -> Self {
        State::SyncingDescendants {
            latest_block: Box::new(latest_block),
            highest_block_seen: 0,
        }
    }

    pub(crate) fn block_downloaded(&mut self, block: &BlockHeader) {
        match self {
            State::None | State::Done => {}
            State::SyncingTrustedHash {
                highest_block_seen, ..
            }
            | State::SyncingDescendants {
                highest_block_seen, ..
            } => {
                let curr_height = block.height();
                if curr_height > *highest_block_seen {
                    *highest_block_seen = curr_height;
                }
            }
        };
    }
}
