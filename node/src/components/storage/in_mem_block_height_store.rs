use std::{
    collections::{btree_map::Entry, BTreeMap},
    fmt::Debug,
    sync::RwLock,
};

use super::{BlockHeightStore, Result};

/// In-memory version of a store.
#[derive(Debug)]
pub(super) struct InMemBlockHeightStore<H> {
    inner: RwLock<BTreeMap<u64, H>>,
}

impl<H> InMemBlockHeightStore<H> {
    pub(crate) fn new() -> Self {
        InMemBlockHeightStore {
            inner: RwLock::new(BTreeMap::new()),
        }
    }
}

impl<H: Send + Sync + Clone> BlockHeightStore<H> for InMemBlockHeightStore<H> {
    fn put(&self, height: u64, block_hash: H) -> Result<bool> {
        if let Entry::Vacant(entry) = self.inner.write().expect("should lock").entry(height) {
            entry.insert(block_hash);
            return Ok(true);
        }
        Ok(false)
    }

    fn get(&self, height: u64) -> Result<Option<H>> {
        Ok(self
            .inner
            .read()
            .expect("should lock")
            .get(&height)
            .cloned())
    }

    fn highest(&self) -> Result<Option<H>> {
        Ok(self
            .inner
            .read()
            .expect("should lock")
            .values()
            .rev()
            .next()
            .cloned())
    }
}
