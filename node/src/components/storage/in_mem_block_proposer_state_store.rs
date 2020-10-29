use std::sync::RwLock;

use crate::components::{
    block_proposer::BlockProposerState,
    storage::{block_proposer_state_store::BlockProposerStateStore, Result},
};

/// In-memory version of a store.
#[derive(Debug)]
pub(super) struct InMemBlockProposerStateStore {
    inner: RwLock<Option<BlockProposerState>>,
}

impl InMemBlockProposerStateStore {
    pub(crate) fn new() -> Self {
        InMemBlockProposerStateStore {
            inner: RwLock::new(None),
        }
    }
}

impl BlockProposerStateStore for InMemBlockProposerStateStore {
    fn put(&self, block_proposer_state: BlockProposerState) -> Result<()> {
        *self.inner.write().expect("should lock") = Some(block_proposer_state);
        Ok(())
    }

    fn get(&self) -> Result<Option<BlockProposerState>> {
        Ok(self.inner.read().expect("should lock").clone())
    }
}
