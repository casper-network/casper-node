use std::fmt::Display;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::{Block, BlockHash, BlockHeader};

#[derive(Clone, DataSize, Debug, Serialize, Deserialize)]
pub enum State {
    /// No syncing of the linear chain configured.
    None,
    /// Synchronizing the linear chain up until trusted hash.
    SyncingTrustedHash {
        /// Linear chain block to start sync from.
        trusted_hash: BlockHash,
        /// The header of the highest block we have in storage (if any).
        highest_block_header: Option<Box<BlockHeader>>,
        /// During synchronization we might see new eras being created.
        /// Track the highest height and wait until it's handled by consensus.
        highest_block_seen: u64,
        /// Chain of downloaded blocks from the linear chain.
        /// We will `pop()` when executing blocks.
        linear_chain: Vec<Block>,
        /// The most recent block we started to execute. This is updated whenever we start
        /// downloading deploys for the next block to be executed.
        latest_block: Box<Option<Block>>,
    },
    /// Synchronizing the descendants of the trusted hash.
    SyncingDescendants {
        trusted_hash: BlockHash,
        /// The most recent block we started to execute. This is updated whenever we start
        /// downloading deploys for the next block to be executed.
        latest_block: Box<Block>,
        /// During synchronization we might see new eras being created.
        /// Track the highest height and wait until it's handled by consensus.
        highest_block_seen: u64,
    },
    /// Synchronizing done. The single field contains the highest block seen during the
    /// synchronization process.
    Done(Option<Box<Block>>),
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            State::None => write!(f, "None"),
            State::Done(_) => write!(f, "Done"),
            State::SyncingTrustedHash { trusted_hash, highest_block_seen, .. } => {
                write!(f, "SyncingTrustedHash(trusted_hash={}, highest_block_seen={})", trusted_hash, highest_block_seen)
            },
            State::SyncingDescendants {
                trusted_hash,
                latest_block,
                ..
            } => write!(
                f,
                "SyncingDescendants(trusted_hash={}, latest_block_hash={}, latest_block_height={}, latest_block_era={})",
                trusted_hash,
                latest_block.header().hash(),
                latest_block.header().height(),
                latest_block.header().era_id(),
            ),
        }
    }
}

impl State {
    pub fn sync_trusted_hash(
        trusted_hash: BlockHash,
        highest_block_header: Option<BlockHeader>,
    ) -> Self {
        State::SyncingTrustedHash {
            trusted_hash,
            highest_block_header: highest_block_header.map(Box::new),
            highest_block_seen: 0,
            linear_chain: Vec::new(),
            latest_block: Box::new(None),
        }
    }

    pub fn sync_descendants(trusted_hash: BlockHash, latest_block: Block) -> Self {
        State::SyncingDescendants {
            trusted_hash,
            latest_block: Box::new(latest_block),
            highest_block_seen: 0,
        }
    }

    pub fn block_downloaded(&mut self, block: &Block) {
        match self {
            State::None | State::Done(_) => {}
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

    /// Returns whether in `Done` state.
    pub(crate) fn is_done(&self) -> bool {
        matches!(self, State::Done(_))
    }

    /// Returns whether in `None` state.
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, State::None)
    }
}
