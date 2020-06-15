//! This module provides a data structure for keeping the finalized blocks.

use std::collections::BTreeMap;

/// An identifier for the block position in the chain.
pub(crate) type BlockIndex = u64;

/// A simple structure to contain a linear progression of blocks (allowing for sparse population,
/// i.e. empty spaces left between blocks)
#[derive(Debug, Default)]
pub(crate) struct Chain<B> {
    blocks: BTreeMap<BlockIndex, B>,
    next_block: BlockIndex,
}

impl<B> Chain<B> {
    /// Creates a new, empty instance of Chain
    pub(crate) fn new() -> Self {
        Self {
            blocks: Default::default(),
            next_block: 0,
        }
    }

    /// Appends a new block right after the last one out of all currently held.
    pub(crate) fn append(&mut self, block: B) -> BlockIndex {
        self.blocks.insert(self.next_block, block);
        let result = self.next_block;
        self.next_block += 1;
        result
    }

    /// Inserts a new block at a given index. Returns the block that was already at this index, if
    /// any.
    /// An Err() result means that the block wasn't the next one supposed to be inserted; the next
    /// expected index is returned along with the block
    pub(crate) fn insert(&mut self, index: BlockIndex, block: B) -> Result<Option<B>, BlockIndex> {
        if index > self.next_block {
            return Err(self.next_block);
        }
        let result = self.blocks.insert(index, block);
        self.next_block += 1;
        Ok(result)
    }

    /// Returns the reference to the last known block.
    pub(crate) fn get_last_block(&self) -> Option<&B> {
        self.blocks.get(&(self.next_block - 1))
    }

    /// Gets the block at a given index.
    pub(crate) fn get_block(&self, index: BlockIndex) -> Option<&B> {
        self.blocks.get(&index)
    }

    /// Returns the current length of the chain (if there are empty spaces in the middle, they are
    /// treated as if there are actual blocks there, but we just don't know them yet)
    pub(crate) fn num_blocks(&self) -> usize {
        self.next_block as usize
    }

    /// Returns an iterator over all the blocks along with their indices
    pub(crate) fn blocks_iterator(&self) -> impl Iterator<Item = (&BlockIndex, &B)> {
        self.blocks.iter()
    }
}
