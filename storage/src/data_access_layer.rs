use casper_types::{execution::Effects, Digest, EraId};

use crate::global_state::{
    error::Error as GlobalStateError,
    state::{CommitProvider, StateProvider},
    trie_store::operations::PruneResult,
};

use crate::tracking_copy::TrackingCopy;

mod addressable_entity;
pub mod balance;
pub mod era_validators;
mod execution_results_checksum;
mod flush;
mod genesis;
pub mod get_all_values;
pub mod get_bids;
mod protocol_upgrade;
pub mod query;
mod round_seigniorage;
mod total_supply;
mod trie;

pub use addressable_entity::{AddressableEntityRequest, AddressableEntityResult};
pub use balance::{BalanceRequest, BalanceResult};
pub use era_validators::{EraValidatorsRequest, EraValidatorsResult};
pub use execution_results_checksum::{
    ExecutionResultsChecksumRequest, ExecutionResultsChecksumResult,
    EXECUTION_RESULTS_CHECKSUM_NAME,
};
pub use flush::{FlushRequest, FlushResult};
pub use genesis::{GenesisRequest, GenesisResult};
pub use get_bids::{BidsRequest, BidsResult};
pub use protocol_upgrade::{ProtocolUpgradeRequest, ProtocolUpgradeResult};
pub use query::{QueryRequest, QueryResult};
pub use round_seigniorage::{RoundSeigniorageRateRequest, RoundSeigniorageRateResult};
pub use total_supply::{TotalSupplyRequest, TotalSupplyResult};
pub use trie::{PutTrieRequest, PutTrieResult, TrieElement, TrieRequest, TrieResult};

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

    fn flush(&self, request: FlushRequest) -> FlushResult {
        self.state.flush(request)
    }

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

    fn trie(&self, request: TrieRequest) -> TrieResult {
        self.state.trie(request)
    }

    fn put_trie(&self, request: PutTrieRequest) -> PutTrieResult {
        self.state.put_trie(request)
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
