use std::{
    collections::{hash_map::Entry, HashMap},
    sync::RwLock,
};

use super::BlockType;

// Trait defining the API for the linear block store.  Doesn't need to fit the "Event/Effect" model
// as this is a sub-component of the storage component.
pub(crate) trait BlockStoreType {
    type Block: BlockType;

    fn put(&self, block: Self::Block) -> bool;
    fn get(
        &self,
        name: &<<Self as BlockStoreType>::Block as BlockType>::Name,
    ) -> Option<Self::Block>;
}

// In-memory version of a block store.
#[derive(Debug)]
pub(crate) struct InMemBlockStore<B: BlockType> {
    inner: RwLock<HashMap<B::Name, B>>,
}

impl<B: BlockType> InMemBlockStore<B> {
    pub(crate) fn new() -> Self {
        InMemBlockStore {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl<B: BlockType> BlockStoreType for InMemBlockStore<B> {
    type Block = B;

    fn put(&self, block: B) -> bool {
        let mut inner = self.inner.write().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Entry::Vacant(entry) = inner.entry(*block.name()) {
            entry.insert(block);
            return true;
        }
        false
    }

    fn get(&self, name: &B::Name) -> Option<Self::Block> {
        self.inner.read().unwrap().get(name).cloned()
    }
}
