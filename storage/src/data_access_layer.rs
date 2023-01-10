use casper_types::EraId;

use crate::global_state::{
    shared,
    storage::{
        state::{CommitProvider, StateProvider},
        trie::TrieRaw,
    },
};

pub struct Block {
    _era_id: EraId,
}

pub trait BlockProvider {
    type Error;

    fn read_block_by_height(&self, _height: usize) -> Result<Option<Block>, Self::Error> {
        // TODO: We need to implement this
        todo!()
    }
}

#[derive(Default)]
pub struct BlockStore(());

impl BlockStore {
    pub fn new() -> Self {
        BlockStore(())
    }
}

// We're currently putting it here, but in future it needs to move to its own crate.
pub struct DataAccessLayer<S> {
    pub block_store: BlockStore,
    pub state: S,
}

impl<S> DataAccessLayer<S> {
    pub fn state(&self) -> &S {
        &self.state
    }
}

impl<S> StateProvider for DataAccessLayer<S>
where
    S: StateProvider,
{
    type Error = S::Error;

    type Reader = S::Reader;

    fn checkout(
        &self,
        state_hash: casper_hashing::Digest,
    ) -> Result<Option<Self::Reader>, Self::Error> {
        self.state.checkout(state_hash)
    }

    fn empty_root(&self) -> casper_hashing::Digest {
        self.state.empty_root()
    }

    fn get_trie_full(
        &self,
        correlation_id: shared::CorrelationId,
        trie_key: &casper_hashing::Digest,
    ) -> Result<Option<TrieRaw>, Self::Error> {
        self.state.get_trie_full(correlation_id, trie_key)
    }

    fn put_trie(
        &self,
        correlation_id: shared::CorrelationId,
        trie: &[u8],
    ) -> Result<casper_hashing::Digest, Self::Error> {
        self.state.put_trie(correlation_id, trie)
    }

    fn missing_children(
        &self,
        correlation_id: shared::CorrelationId,
        trie_raw: &[u8],
    ) -> Result<Vec<casper_hashing::Digest>, Self::Error> {
        self.state.missing_children(correlation_id, trie_raw)
    }
}

impl<S> CommitProvider for DataAccessLayer<S>
where
    S: CommitProvider,
{
    fn commit(
        &self,
        correlation_id: shared::CorrelationId,
        state_hash: casper_hashing::Digest,
        effects: shared::AdditiveMap<casper_types::Key, shared::transform::Transform>,
    ) -> Result<casper_hashing::Digest, Self::Error> {
        self.state.commit(correlation_id, state_hash, effects)
    }
}
