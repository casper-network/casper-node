use casper_types::{Digest, EraId};

use crate::global_state::{
    shared,
    state::{CommitProvider, StateProvider},
    trie::TrieRaw,
    trie_store::operations::PruneResult,
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

    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, Self::Error> {
        self.state.checkout(state_hash)
    }

    fn empty_root(&self) -> Digest {
        self.state.empty_root()
    }

    fn get_trie_full(&self, trie_key: &Digest) -> Result<Option<TrieRaw>, Self::Error> {
        self.state.get_trie_full(trie_key)
    }

    fn put_trie(&self, trie: &[u8]) -> Result<Digest, Self::Error> {
        self.state.put_trie(trie)
    }

    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, Self::Error> {
        self.state.missing_children(trie_raw)
    }

    fn prune_keys(
        &self,
        root: Digest,
        keys_to_prune: &[casper_types::Key],
    ) -> Result<PruneResult, Self::Error> {
        self.state.prune_keys(root, keys_to_prune)
    }
}

impl<S> CommitProvider for DataAccessLayer<S>
where
    S: CommitProvider,
{
    fn commit(
        &self,
        state_hash: Digest,
        effects: shared::AdditiveMap<casper_types::Key, shared::transform::Transform>,
    ) -> Result<Digest, Self::Error> {
        self.state.commit(state_hash, effects)
    }
}
