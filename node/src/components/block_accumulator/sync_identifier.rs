use std::fmt::{Display, Formatter};

use casper_types::{BlockHash, EraId};

#[derive(Clone, Debug)]
pub(crate) enum SyncIdentifier {
    // all we know about the block is its hash;
    // this is usually a trusted hash from config
    BlockHash(BlockHash),
    // we know both the hash and the height of the block
    BlockIdentifier(BlockHash, u64),
    // we have just acquired the necessary data for the block
    // including sufficient finality; this may be a historical
    // block and / or potentially the new highest block
    SyncedBlockIdentifier(BlockHash, u64, EraId),
    // we acquired the necessary data for the block, including
    // sufficient finality and it has been enqueued for
    // execution; this state is valid for forward blocks only
    ExecutingBlockIdentifier(BlockHash, u64, EraId),
    // we read this block from disk, and have all the parts
    // we need to discover its descendent (if any) to continue.
    LocalTip(BlockHash, u64, EraId),
}

impl SyncIdentifier {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            SyncIdentifier::BlockIdentifier(hash, _)
            | SyncIdentifier::SyncedBlockIdentifier(hash, _, _)
            | SyncIdentifier::ExecutingBlockIdentifier(hash, _, _)
            | SyncIdentifier::LocalTip(hash, _, _)
            | SyncIdentifier::BlockHash(hash) => *hash,
        }
    }

    pub(crate) fn block_height(&self) -> Option<u64> {
        match self {
            SyncIdentifier::BlockIdentifier(_, height)
            | SyncIdentifier::SyncedBlockIdentifier(_, height, _)
            | SyncIdentifier::ExecutingBlockIdentifier(_, height, _)
            | SyncIdentifier::LocalTip(_, height, _) => Some(*height),
            SyncIdentifier::BlockHash(_) => None,
        }
    }

    pub(crate) fn era_id(&self) -> Option<EraId> {
        match self {
            SyncIdentifier::BlockHash(_) | SyncIdentifier::BlockIdentifier(_, _) => None,
            SyncIdentifier::SyncedBlockIdentifier(_, _, era_id)
            | SyncIdentifier::ExecutingBlockIdentifier(_, _, era_id)
            | SyncIdentifier::LocalTip(_, _, era_id) => Some(*era_id),
        }
    }

    pub(crate) fn block_height_and_era(&self) -> Option<(u64, EraId)> {
        if let (Some(block_height), Some(era_id)) = (self.block_height(), self.era_id()) {
            return Some((block_height, era_id));
        }
        None
    }

    pub(crate) fn is_held_locally(&self) -> bool {
        match self {
            SyncIdentifier::BlockHash(_) | SyncIdentifier::BlockIdentifier(_, _) => false,

            SyncIdentifier::SyncedBlockIdentifier(_, _, _)
            | SyncIdentifier::ExecutingBlockIdentifier(_, _, _)
            | SyncIdentifier::LocalTip(_, _, _) => true,
        }
    }

    pub(crate) fn block_hash_to_sync(&self, child_hash: Option<BlockHash>) -> Option<BlockHash> {
        if self.is_held_locally() {
            child_hash
        } else {
            Some(self.block_hash())
        }
    }
}

impl Display for SyncIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncIdentifier::BlockHash(block_hash) => block_hash.fmt(f),
            SyncIdentifier::BlockIdentifier(block_hash, block_height) => {
                write!(
                    f,
                    "block_hash: {} block_height: {}",
                    block_hash, block_height
                )
            }
            SyncIdentifier::SyncedBlockIdentifier(block_hash, block_height, era_id)
            | SyncIdentifier::ExecutingBlockIdentifier(block_hash, block_height, era_id)
            | SyncIdentifier::LocalTip(block_hash, block_height, era_id) => {
                write!(
                    f,
                    "block_hash: {} block_height: {} era_id: {}",
                    block_hash, block_height, era_id
                )
            }
        }
    }
}
