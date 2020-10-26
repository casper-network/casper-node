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

    use crate::testing::TestRng;

    fn should_put_then_get<T: BlockProposerStateStore>(block_proposer_state_store: &mut T) {
        let mut rng = TestRng::new();

        block_proposer_state_store.put().unwrap();
        let maybe_block_proposer_state = block_proposer_state_store.get(version).unwrap();
        let recovered_block_proposer_state = maybe_block_proposer_state.unwrap();

        assert_eq!(recovered_block_proposer_state, block_proposer_state);
    }

    #[test]
    fn lmdb_block_proposer_state_store_should_put_then_get() {
        let (config, _tempdir) = Config::default_for_tests();
        let mut lmdb_block_proposer_state_store =
            LmdbBlockProposerStore::new(config.path(), config.max_block_proposer_state_store_size()).unwrap();
        should_put_then_get(&mut lmdb_block_proposer_state_store);
    }

    #[test]
    fn in_mem_block_proposer_state_store_should_put_then_get() {
        let mut in_mem_block_proposer_state_store = InMemBlockProposerStateStore::new();
        should_put_then_get(&mut in_mem_block_proposer_state_store);
    }

    fn should_fail_get<T: BlockProposerStore>(block_proposer_state_store: &mut T) {
        let mut rng = TestRng::new();

        let block_proposer_state = BlockProposer::random(&mut rng);
        let mut version = block_proposer_state.genesis.protocol_version.clone();
        version.patch += 1;

        block_proposer_state_store.put(block_proposer_state).unwrap();
        assert!(block_proposer_state_store.get(version).unwrap().is_none());
    }

    #[test]
    fn lmdb_block_proposer_state_store_should_fail_to_get_unknown_version() {
        let (config, _tempdir) = Config::default_for_tests();
        let mut lmdb_block_proposer_state_store =
            LmdbBlockProposerStore::new(config.path(), config.max_block_proposer_state_store_size()).unwrap();
        should_fail_get(&mut lmdb_block_proposer_state_store);
    }

    #[test]
    fn in_mem_block_proposer_state_store_should_fail_to_get_unknown_version() {
        let mut in_mem_block_proposer_state_store = InMemBlockProposerStore::new();
        should_fail_get(&mut in_mem_block_proposer_state_store);
    }
}
