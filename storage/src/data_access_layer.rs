use casper_types::{execution::Effects, Digest, EraId};

use crate::global_state::{
    error::Error as GlobalStateError,
    state::{CommitProvider, StateProvider},
    trie::TrieRaw,
    trie_store::operations::PruneResult,
};

use crate::tracking_copy::TrackingCopy;

pub mod balance;
pub mod get_bids;
pub mod query;

pub use balance::{BalanceRequest, BalanceResult};
pub use get_bids::{GetBidsError, GetBidsRequest, GetBidsResult};
pub use query::{QueryRequest, QueryResult};

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

#[derive(Default, Copy, Clone)]
pub struct BlockStore(());

impl BlockStore {
    pub fn new() -> Self {
        BlockStore(())
    }
}

// We're currently putting it here, but in future it needs to move to its own crate.
#[derive(Copy, Clone)]
pub struct DataAccessLayer<S> {
    pub block_store: BlockStore,
    pub state: S,
    pub max_query_depth: u64,
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
    type Reader = S::Reader;

    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, GlobalStateError> {
        self.state.checkout(state_hash)
    }

    fn tracking_copy(
        &self,
        hash: Digest,
    ) -> Result<Option<TrackingCopy<S::Reader>>, GlobalStateError> {
        match self.state.checkout(hash)? {
            Some(reader) => Ok(Some(TrackingCopy::new(reader, self.max_query_depth))),
            None => Ok(None),
        }
    }

    fn empty_root(&self) -> Digest {
        self.state.empty_root()
    }

    fn get_trie_full(&self, trie_key: &Digest) -> Result<Option<TrieRaw>, GlobalStateError> {
        self.state.get_trie_full(trie_key)
    }

    fn put_trie(&self, trie: &[u8]) -> Result<Digest, GlobalStateError> {
        self.state.put_trie(trie)
    }

    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, GlobalStateError> {
        self.state.missing_children(trie_raw)
    }

    fn prune_keys(
        &self,
        root: Digest,
        keys_to_prune: &[casper_types::Key],
    ) -> Result<PruneResult, GlobalStateError> {
        self.state.prune_keys(root, keys_to_prune)
    }
}

impl<S> CommitProvider for DataAccessLayer<S>
where
    S: CommitProvider,
{
    fn commit(&self, state_hash: Digest, effects: Effects) -> Result<Digest, GlobalStateError> {
        self.state.commit(state_hash, effects)
    }
}
