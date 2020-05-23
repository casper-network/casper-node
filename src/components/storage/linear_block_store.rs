use std::collections::{hash_map::Entry, HashMap};

use super::BlockType;

// Trait defining the API for the linear block store.  Doesn't need to fit the "Event/Effect" model
// as this is a sub-component of the storage component.
pub(crate) trait BlockStoreType {
    type Block: BlockType;

    fn put(&mut self, block: Self::Block) -> bool;
    fn get(
        &self,
        name: &<<Self as BlockStoreType>::Block as BlockType>::Name,
    ) -> Option<Self::Block>;
}

// In-memory version of a block store.
#[derive(Debug)]
pub(crate) struct InMemBlockStore<B: BlockType> {
    inner: HashMap<B::Name, B>,
}

impl<B: BlockType> InMemBlockStore<B> {
    pub(crate) fn new() -> Self {
        InMemBlockStore {
            inner: HashMap::new(),
        }
    }
}

impl<B: BlockType> BlockStoreType for InMemBlockStore<B> {
    type Block = B;

    fn put(&mut self, block: B) -> bool {
        if let Entry::Vacant(entry) = self.inner.entry(*block.name()) {
            entry.insert(block);
            return true;
        }
        false
    }

    fn get(&self, name: &B::Name) -> Option<Self::Block> {
        self.inner.get(name).cloned()
    }
}
