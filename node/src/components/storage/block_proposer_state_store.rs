use super::Result;
use crate::components::block_proposer::BlockProposerState;

/// Trait defining the API for a block_proposer_state store managed by the storage component.
pub trait BlockProposerStateStore: Send + Sync {
    fn put(&self, block_proposer_state: BlockProposerState) -> Result<()>;
    fn get(&self) -> Result<Option<BlockProposerState>>;
}

pub(super) const BLOCK_PROPOSER_STATE_KEY: &str = "BLOCK_PROPOSER_STATE";

#[cfg(test)]
mod tests {
    use super::{
        super::{Config, InMemBlockProposerStateStore, LmdbBlockProposerStateStore},
        *,
    };

    fn should_put_then_get<T: BlockProposerStateStore>(block_proposer_state_store: &mut T) {
        let expected = BlockProposerState::default();
        block_proposer_state_store.put(expected.clone()).unwrap();
        let maybe_block_proposer_state = block_proposer_state_store.get().unwrap();
        let recovered_block_proposer_state = maybe_block_proposer_state.unwrap();
        assert_eq!(recovered_block_proposer_state, expected);
    }

    #[test]
    fn lmdb_block_proposer_state_store_should_put_then_get() {
        let (config, _tempdir) = Config::default_for_tests();
        let mut lmdb_block_proposer_state_store = LmdbBlockProposerStateStore::new(
            config.path(),
            config.max_block_proposer_state_store_size(),
        )
        .unwrap();
        should_put_then_get(&mut lmdb_block_proposer_state_store);
    }

    #[test]
    fn in_mem_block_proposer_state_store_should_put_then_get() {
        let mut in_mem_block_proposer_state_store = InMemBlockProposerStateStore::new();
        should_put_then_get(&mut in_mem_block_proposer_state_store);
    }
}
