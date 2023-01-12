use casper_types::EraId;

use crate::types::BlockHash;

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

    pub(crate) fn is_held_locally(&self) -> bool {
        match self {
            SyncIdentifier::BlockHash(_) | SyncIdentifier::BlockIdentifier(_, _) => false,

            SyncIdentifier::SyncedBlockIdentifier(_, _, _)
            | SyncIdentifier::ExecutingBlockIdentifier(_, _, _)
            | SyncIdentifier::LocalTip(_, _, _) => true,
        }
    }

    pub(crate) fn maybe_local_tip_identifier(&self) -> Option<(u64, EraId)> {
        match self {
            SyncIdentifier::BlockHash(_)
            | SyncIdentifier::BlockIdentifier(_, _)
            | SyncIdentifier::ExecutingBlockIdentifier(_, _, _) => None,

            SyncIdentifier::SyncedBlockIdentifier(_, block_height, era_id)
            | SyncIdentifier::LocalTip(_, block_height, era_id) => Some((*block_height, *era_id)),
        }
    }
}
