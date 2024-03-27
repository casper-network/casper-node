use crate::global_state::{
    error::Error as GlobalStateError,
    state::{CommitProvider, StateProvider},
};
use casper_types::{execution::Effects, Digest, EraId};

use crate::tracking_copy::TrackingCopy;

mod addressable_entity;
pub mod balance;
pub mod bidding;
pub mod bids;
pub mod block_rewards;
pub mod era_validators;
mod execution_results_checksum;
mod fee;
mod flush;
pub mod forced_undelegate;
mod genesis;
mod protocol_upgrade;
pub mod prune;
pub mod query;
mod round_seigniorage;
pub mod step;
mod system_entity_registry;
pub mod tagged_values;
mod total_supply;
pub mod transfer;
mod trie;

pub use addressable_entity::{AddressableEntityRequest, AddressableEntityResult};
pub use balance::{BalanceIdentifier, BalanceRequest, BalanceResult};
pub use bidding::{BiddingRequest, BiddingResult};
pub use bids::{BidsRequest, BidsResult};
pub use block_rewards::{BlockRewardsError, BlockRewardsRequest, BlockRewardsResult};
pub use era_validators::{EraValidatorsRequest, EraValidatorsResult};
pub use execution_results_checksum::{
    ExecutionResultsChecksumRequest, ExecutionResultsChecksumResult,
    EXECUTION_RESULTS_CHECKSUM_NAME,
};
pub use fee::{FeeError, FeeRequest, FeeResult};
pub use flush::{FlushRequest, FlushResult};
pub use genesis::{GenesisRequest, GenesisResult};
pub use protocol_upgrade::{ProtocolUpgradeRequest, ProtocolUpgradeResult};
pub use prune::{PruneRequest, PruneResult};
pub use query::{QueryRequest, QueryResult};
pub use round_seigniorage::{RoundSeigniorageRateRequest, RoundSeigniorageRateResult};
pub use step::{EvictItem, RewardItem, SlashItem, StepError, StepRequest, StepResult};
pub use system_entity_registry::{
    SystemEntityRegistryPayload, SystemEntityRegistryRequest, SystemEntityRegistryResult,
    SystemEntityRegistrySelector,
};
pub use total_supply::{TotalSupplyRequest, TotalSupplyResult};
pub use transfer::{TransferRequest, TransferResult};
pub use trie::{PutTrieRequest, PutTrieResult, TrieElement, TrieRequest, TrieResult};

pub use bidding::AuctionMethod;

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

impl<S> CommitProvider for DataAccessLayer<S>
where
    S: CommitProvider,
{
    fn commit(&self, state_hash: Digest, effects: Effects) -> Result<Digest, GlobalStateError> {
        self.state.commit(state_hash, effects)
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
}
