use std::{collections::BTreeMap, fmt::Display};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::{BlockHash, BlockHeader};
use casper_types::{PublicKey, U512};

#[derive(Clone, DataSize, Debug, Serialize, Deserialize)]
pub enum State {
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
        /// The weights of the validators for latest block being added.
        validator_weights: BTreeMap<PublicKey, U512>,
    },
    /// Synchronizing the descendants of the trusted hash.
    SyncingDescendants {
        trusted_hash: BlockHash,
        /// The most recent block we started to execute. This is updated whenever we start
        /// downloading deploys for the next block to be executed.
        latest_block: Box<BlockHeader>,
        /// During synchronization we might see new eras being created.
        /// Track the highest height and wait until it's handled by consensus.
        highest_block_seen: u64,
        /// The validator set for the most recent block being synchronized.
        validators_for_latest_block: BTreeMap<PublicKey, U512>,
    },
    /// Synchronizing done.
    Done,
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            State::None => write!(f, "None"),
            State::Done => write!(f, "Done"),
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
            latest_block.hash(),
            latest_block.height(),
            latest_block.era_id(),
            ),
        }
    }
}

impl State {
    pub fn sync_trusted_hash(
        trusted_hash: BlockHash,
        validator_weights: BTreeMap<PublicKey, U512>,
    ) -> Self {
        State::SyncingTrustedHash {
            trusted_hash,
            highest_block_seen: 0,
            linear_chain: Vec::new(),
            latest_block: Box::new(None),
            validator_weights,
        }
    }

    pub fn sync_descendants(
        trusted_hash: BlockHash,
        latest_block: BlockHeader,
        validators_for_latest_block: BTreeMap<PublicKey, U512>,
    ) -> Self {
        State::SyncingDescendants {
            trusted_hash,
            latest_block: Box::new(latest_block),
            highest_block_seen: 0,
            validators_for_latest_block,
        }
    }

    pub fn block_downloaded(&mut self, block: &BlockHeader) {
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

    /// Returns whether in `Done` state.
    pub(crate) fn is_done(&self) -> bool {
        matches!(self, State::Done)
    }

    /// Returns whether in `None` state.
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, State::None)
    }
}
