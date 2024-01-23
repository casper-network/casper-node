use casper_types::{Block, BlockHeader};

use super::error::BlockStoreError;

/// A trait for representing indexed access to a block store.
pub trait IndexedBy<Key, Value> {
    /// Iterator over the indexed keys.
    type KeysIterator: Iterator<Item = Key>;

    /// Get the value from the block store corresponding with the provided Key.
    fn get(&mut self, key: &Key) -> Result<Option<Value>, BlockStoreError>;

    /// Get an iterator for the index keys.
    fn keys(&self) -> Self::KeysIterator;
}

pub type BlockHeight = u64;
pub type SwitchBlockHeader = BlockHeader;
pub type SwitchBlock = Block;
