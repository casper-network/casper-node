use crate::global_state::{
    error::Error as GlobalStateError,
    state::{CommitProvider, StateProvider},
};
use casper_types::{execution::Effects, Digest};

use crate::tracking_copy::TrackingCopy;

mod addressable_entity;
/// Auction provider.
pub mod auction;
/// Balance provider.
pub mod balance;
mod balance_hold;
mod balance_identifier_purse;
/// Bids provider.
pub mod bids;
mod block_global;
/// Block rewards provider.
pub mod block_rewards;
mod contract;
mod entry_points;
/// Era validators provider.
pub mod era_validators;
mod execution_results_checksum;
mod fee;
mod flush;
/// Forced undelegate provider.
pub mod forced_undelegate;
mod genesis;
/// Handle fee provider.
pub mod handle_fee;
mod handle_refund;
mod key_prefix;
/// Message topics.
pub mod message_topics;
/// Mint provider.
pub mod mint;
/// Prefixed values provider.
pub mod prefixed_values;
mod protocol_upgrade;
/// Prune provider.
pub mod prune;
/// Query provider.
pub mod query;
mod round_seigniorage;
mod seigniorage_recipients;
/// Step provider.
pub mod step;
mod system_entity_registry;
/// Tagged values provider.
pub mod tagged_values;
mod total_supply;
mod trie;

pub use addressable_entity::{AddressableEntityRequest, AddressableEntityResult};
pub use auction::{AuctionMethod, BiddingRequest, BiddingResult};
pub use balance::{
    BalanceHolds, BalanceHoldsWithProof, BalanceIdentifier, BalanceRequest, BalanceResult,
    GasHoldBalanceHandling, ProofHandling, ProofsResult,
};
pub use balance_hold::{
    BalanceHoldError, BalanceHoldKind, BalanceHoldMode, BalanceHoldRequest, BalanceHoldResult,
    InsufficientBalanceHandling,
};
pub use balance_identifier_purse::{BalanceIdentifierPurseRequest, BalanceIdentifierPurseResult};
pub use bids::{BidsRequest, BidsResult};
pub use block_global::{BlockGlobalKind, BlockGlobalRequest, BlockGlobalResult};
pub use block_rewards::{BlockRewardsError, BlockRewardsRequest, BlockRewardsResult};
pub use contract::{ContractRequest, ContractResult};
pub use entry_points::{
    EntryPointExistsRequest, EntryPointExistsResult, EntryPointRequest, EntryPointResult,
};
pub use era_validators::{EraValidatorsRequest, EraValidatorsResult};
pub use execution_results_checksum::{
    ExecutionResultsChecksumRequest, ExecutionResultsChecksumResult,
    EXECUTION_RESULTS_CHECKSUM_NAME,
};
pub use fee::{FeeError, FeeRequest, FeeResult};
pub use flush::{FlushRequest, FlushResult};
pub use genesis::{GenesisRequest, GenesisResult};
pub use handle_fee::{HandleFeeMode, HandleFeeRequest, HandleFeeResult};
pub use handle_refund::{HandleRefundMode, HandleRefundRequest, HandleRefundResult};
pub use key_prefix::KeyPrefix;
pub use message_topics::{MessageTopicsRequest, MessageTopicsResult};
pub use mint::{TransferRequest, TransferResult};
pub use protocol_upgrade::{ProtocolUpgradeRequest, ProtocolUpgradeResult};
pub use prune::{PruneRequest, PruneResult};
pub use query::{QueryRequest, QueryResult};
pub use round_seigniorage::{RoundSeigniorageRateRequest, RoundSeigniorageRateResult};
pub use seigniorage_recipients::{SeigniorageRecipientsRequest, SeigniorageRecipientsResult};
pub use step::{EvictItem, RewardItem, SlashItem, StepError, StepRequest, StepResult};
pub use system_entity_registry::{
    SystemEntityRegistryPayload, SystemEntityRegistryRequest, SystemEntityRegistryResult,
    SystemEntityRegistrySelector,
};
pub use total_supply::{TotalSupplyRequest, TotalSupplyResult};
pub use trie::{PutTrieRequest, PutTrieResult, TrieElement, TrieRequest, TrieResult};

/// Anchor struct for block store functionality.
#[derive(Default, Copy, Clone)]
pub struct BlockStore(());

impl BlockStore {
    /// Ctor.
    pub fn new() -> Self {
        BlockStore(())
    }
}

/// Data access layer.
#[derive(Copy, Clone)]
pub struct DataAccessLayer<S> {
    /// Block store instance.
    pub block_store: BlockStore,
    /// Memoized state.
    pub state: S,
    /// Max query depth.
    pub max_query_depth: u64,
    /// Enable the addressable entity capability.
    pub enable_addressable_entity: bool,
}

impl<S> DataAccessLayer<S> {
    /// Returns reference to current state of the data access layer.
    pub fn state(&self) -> &S {
        &self.state
    }
}

impl<S> CommitProvider for DataAccessLayer<S>
where
    S: CommitProvider,
{
    fn commit_effects(
        &self,
        state_hash: Digest,
        effects: Effects,
    ) -> Result<Digest, GlobalStateError> {
        self.state.commit_effects(state_hash, effects)
    }

    fn commit_values(
        &self,
        state_hash: Digest,
        values_to_write: Vec<(casper_types::Key, casper_types::StoredValue)>,
        keys_to_prune: std::collections::BTreeSet<casper_types::Key>,
    ) -> Result<Digest, GlobalStateError> {
        self.state
            .commit_values(state_hash, values_to_write, keys_to_prune)
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

    fn empty_root(&self) -> Digest {
        self.state.empty_root()
    }

    fn tracking_copy(
        &self,
        hash: Digest,
    ) -> Result<Option<TrackingCopy<S::Reader>>, GlobalStateError> {
        match self.state.checkout(hash)? {
            Some(reader) => Ok(Some(TrackingCopy::new(
                reader,
                self.max_query_depth,
                self.enable_addressable_entity,
            ))),
            None => Ok(None),
        }
    }

    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, GlobalStateError> {
        self.state.checkout(state_hash)
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

    fn enable_entity(&self) -> bool {
        self.state.enable_entity()
    }
}
