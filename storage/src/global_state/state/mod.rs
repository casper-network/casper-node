//! Global state.

/// Lmdb implementation of global state.
pub mod lmdb;

/// Lmdb implementation of global state with cache.
pub mod scratch;

use num_rational::Ratio;
use parking_lot::RwLock;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    rc::Rc,
    sync::Arc,
};

use tracing::{debug, error, info, warn};

use casper_types::{
    account::AccountHash,
    bytesrepr::{self, ToBytes},
    contracts::NamedKeys,
    execution::{Effects, TransformError, TransformInstruction, TransformKindV2, TransformV2},
    global_state::TrieMerkleProof,
    system::{
        self,
        auction::{
            BidAddrTag, BidKind, DelegatorBid, DelegatorKind, SeigniorageRecipientsSnapshot,
            Unbond, UnbondEra, UnbondKind, UnbondingPurse, ERA_END_TIMESTAMP_MILLIS_KEY,
            ERA_ID_KEY, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY,
        },
        mint::{
            BalanceHoldAddr, BalanceHoldAddrTag, ARG_AMOUNT, ROUND_SEIGNIORAGE_RATE_KEY,
            TOTAL_SUPPLY_KEY,
        },
        AUCTION, HANDLE_PAYMENT, MINT,
    },
    Account, AddressableEntity, BlockGlobalAddr, CLValue, Digest, EntityAddr, EntityEntryPoint,
    EntryPointAddr, EntryPointValue, HoldsEpoch, Key, KeyTag, Phase, PublicKey, RuntimeArgs,
    StoredValue, SystemHashRegistry, U512,
};

#[cfg(test)]
pub use self::lmdb::make_temporary_global_state;

use super::trie_store::{operations::batch_write, TrieStoreCacheError};
use crate::{
    data_access_layer::{
        auction::{AuctionMethodRet, BiddingRequest, BiddingResult},
        balance::BalanceHandling,
        bids::{
            DelegatorBidRequest, DelegatorBidsResult, ValidatorBidRequest, ValidatorBidsResult,
        },
        era_validators::EraValidatorsResult,
        handle_fee::{HandleFeeMode, HandleFeeRequest, HandleFeeResult},
        mint::{
            BurnRequest, BurnRequestArgs, BurnResult, TransferRequest, TransferRequestArgs,
            TransferResult,
        },
        prefixed_values::{PrefixedValuesRequest, PrefixedValuesResult},
        tagged_values::{TaggedValuesRequest, TaggedValuesResult},
        AddressableEntityRequest, AddressableEntityResult, AuctionMethod, BalanceHoldError,
        BalanceHoldKind, BalanceHoldMode, BalanceHoldRequest, BalanceHoldResult, BalanceIdentifier,
        BalanceIdentifierPurseRequest, BalanceIdentifierPurseResult, BalanceRequest, BalanceResult,
        BidsRequest, BidsResult, BlockGlobalKind, BlockGlobalRequest, BlockGlobalResult,
        BlockRewardsError, BlockRewardsRequest, BlockRewardsResult, ContractRequest,
        ContractResult, EntryPointExistsRequest, EntryPointExistsResult, EntryPointRequest,
        EntryPointResult, EraValidatorsRequest, ExecutionResultsChecksumRequest,
        ExecutionResultsChecksumResult, FeeError, FeeRequest, FeeResult, FlushRequest, FlushResult,
        GenesisRequest, GenesisResult, HandleRefundMode, HandleRefundRequest, HandleRefundResult,
        InsufficientBalanceHandling, MessageTopicsRequest, MessageTopicsResult, ProofHandling,
        ProofsResult, ProtocolUpgradeRequest, ProtocolUpgradeResult, PruneRequest, PruneResult,
        PutTrieRequest, PutTrieResult, QueryRequest, QueryResult, RoundSeigniorageRateRequest,
        RoundSeigniorageRateResult, SeigniorageRecipientsRequest, SeigniorageRecipientsResult,
        StepError, StepRequest, StepResult, SystemEntityRegistryPayload,
        SystemEntityRegistryRequest, SystemEntityRegistryResult, SystemEntityRegistrySelector,
        TotalSupplyRequest, TotalSupplyResult, TrieRequest, TrieResult,
        EXECUTION_RESULTS_CHECKSUM_NAME,
    },
    global_state::{
        error::Error as GlobalStateError,
        state::scratch::ScratchGlobalState,
        transaction_source::{Transaction, TransactionSource},
        trie::Trie,
        trie_store::{
            operations::{prune, read, write, ReadResult, TriePruneResult, WriteResult},
            TrieStore,
        },
    },
    system::{
        auction::{self, Auction},
        burn::{BurnError, BurnRuntimeArgsBuilder},
        genesis::{GenesisError, GenesisInstaller},
        handle_payment::HandlePayment,
        mint::Mint,
        protocol_upgrade::{ProtocolUpgradeError, ProtocolUpgrader},
        runtime_native::{Id, RuntimeNative},
        transfer::{TransferArgs, TransferError, TransferRuntimeArgsBuilder, TransferTargetMode},
    },
    tracking_copy::{TrackingCopy, TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
    AddressGenerator,
};

const BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_VALIDATORS: &[BidAddrTag] = &[
    BidAddrTag::Validator,
    BidAddrTag::DelegatedAccount,
    BidAddrTag::DelegatedPurse,
    BidAddrTag::Credit,
    BidAddrTag::ReservedDelegationAccount,
    BidAddrTag::ReservedDelegationPurse,
    BidAddrTag::UnbondAccount,
    BidAddrTag::UnbondPurse,
];

const BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_ACCOUNT: &[BidAddrTag] = &[
    BidAddrTag::DelegatedAccount,
    BidAddrTag::ReservedDelegationAccount,
    BidAddrTag::UnbondAccount,
];

const BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_PURSE: &[BidAddrTag] = &[
    BidAddrTag::DelegatedPurse,
    BidAddrTag::ReservedDelegationPurse,
    BidAddrTag::UnbondPurse,
];

/// A trait expressing the reading of state. This trait is used to abstract the underlying store.
pub trait StateReader<K = Key, V = StoredValue>: Sized + Send + Sync {
    /// An error which occurs when reading state
    type Error;

    /// Returns the state value from the corresponding key
    fn read(&self, key: &K) -> Result<Option<V>, Self::Error>;

    /// Returns the merkle proof of the state value from the corresponding key
    fn read_with_proof(&self, key: &K) -> Result<Option<TrieMerkleProof<K, V>>, Self::Error>;

    /// Returns the keys in the trie matching `prefix`.
    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<K>, Self::Error>;
}

/// An error emitted by the execution engine on commit
#[derive(Clone, Debug, thiserror::Error, Eq, PartialEq)]
pub enum CommitError {
    /// Root not found.
    #[error("Root not found: {0:?}")]
    RootNotFound(Digest),
    /// Root not found while attempting to read.
    #[error("Root not found while attempting to read: {0:?}")]
    ReadRootNotFound(Digest),
    /// Root not found while attempting to write.
    #[error("Root not found while writing: {0:?}")]
    WriteRootNotFound(Digest),
    /// Key not found.
    #[error("Key not found: {0}")]
    KeyNotFound(Key),
    /// Transform error.
    #[error(transparent)]
    TransformError(TransformError),
    /// Trie not found while attempting to validate cache write.
    #[error("Trie not found in cache {0}")]
    TrieNotFoundInCache(Digest),
}

/// Scratch provider.
pub trait ScratchProvider: CommitProvider {
    /// Get scratch state to db.
    fn get_scratch_global_state(&self) -> ScratchGlobalState;
    /// Write scratch state to db.
    fn write_scratch_to_db(
        &self,
        state_root_hash: Digest,
        scratch_global_state: ScratchGlobalState,
    ) -> Result<Digest, GlobalStateError>;
    /// Prune items for imputed keys.
    fn prune_keys(&self, state_root_hash: Digest, keys: &[Key]) -> TriePruneResult;
}

/// Provides `commit` method.
pub trait CommitProvider: StateProvider {
    /// Applies changes and returns a new post state hash.
    /// block_hash is used for computing a deterministic and unique keys.
    fn commit_effects(
        &self,
        state_hash: Digest,
        effects: Effects,
    ) -> Result<Digest, GlobalStateError>;

    /// Commit values to global state.
    fn commit_values(
        &self,
        state_hash: Digest,
        values_to_write: Vec<(Key, StoredValue)>,
        keys_to_prune: BTreeSet<Key>,
    ) -> Result<Digest, GlobalStateError>;

    /// Runs and commits the genesis process, once per network.
    fn genesis(&self, request: GenesisRequest) -> GenesisResult {
        let initial_root = self.empty_root();
        let tc = match self.tracking_copy(initial_root) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return GenesisResult::Fatal("state uninitialized".to_string()),
            Err(err) => {
                return GenesisResult::Failure(GenesisError::TrackingCopy(
                    TrackingCopyError::Storage(err),
                ));
            }
        };
        let chainspec_hash = request.chainspec_hash();
        let protocol_version = request.protocol_version();
        let config = request.config();

        let mut genesis_installer: GenesisInstaller<Self> =
            GenesisInstaller::new(chainspec_hash, protocol_version, config.clone(), tc);

        let chainspec_registry = request.chainspec_registry();
        if let Err(gen_err) = genesis_installer.install(chainspec_registry.clone()) {
            return GenesisResult::Failure(*gen_err);
        }

        let effects = genesis_installer.finalize();
        match self.commit_effects(initial_root, effects.clone()) {
            Ok(post_state_hash) => GenesisResult::Success {
                post_state_hash,
                effects,
            },
            Err(err) => {
                GenesisResult::Failure(GenesisError::TrackingCopy(TrackingCopyError::Storage(err)))
            }
        }
    }

    /// Runs and commits the protocol upgrade process.
    fn protocol_upgrade(&self, request: ProtocolUpgradeRequest) -> ProtocolUpgradeResult {
        let pre_state_hash = request.pre_state_hash();
        let tc = match self.tracking_copy(pre_state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return ProtocolUpgradeResult::RootNotFound,
            Err(err) => {
                return ProtocolUpgradeResult::Failure(ProtocolUpgradeError::TrackingCopy(
                    TrackingCopyError::Storage(err),
                ));
            }
        };

        let protocol_upgrader: ProtocolUpgrader<Self> =
            ProtocolUpgrader::new(request.config().clone(), pre_state_hash, tc);

        let post_upgrade_tc = match protocol_upgrader.upgrade(pre_state_hash) {
            Err(e) => return e.into(),
            Ok(tc) => tc,
        };

        let (writes, prunes, effects) = post_upgrade_tc.destructure();

        // commit
        match self.commit_values(pre_state_hash, writes, prunes) {
            Ok(post_state_hash) => ProtocolUpgradeResult::Success {
                post_state_hash,
                effects,
            },
            Err(err) => ProtocolUpgradeResult::Failure(ProtocolUpgradeError::TrackingCopy(
                TrackingCopyError::Storage(err),
            )),
        }
    }

    /// Safely prune specified keys from global state, using a tracking copy.
    fn prune(&self, request: PruneRequest) -> PruneResult {
        let pre_state_hash = request.state_hash();
        let tc = match self.tracking_copy(pre_state_hash) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return PruneResult::RootNotFound,
            Err(err) => return PruneResult::Failure(TrackingCopyError::Storage(err)),
        };

        let keys_to_delete = request.keys_to_prune();
        if keys_to_delete.is_empty() {
            // effectively a noop
            return PruneResult::Success {
                post_state_hash: pre_state_hash,
                effects: Effects::default(),
            };
        }

        for key in keys_to_delete {
            tc.borrow_mut().prune(*key)
        }

        let effects = tc.borrow().effects();

        match self.commit_effects(pre_state_hash, effects.clone()) {
            Ok(post_state_hash) => PruneResult::Success {
                post_state_hash,
                effects,
            },
            Err(tce) => PruneResult::Failure(tce.into()),
        }
    }

    /// Step auction state at era end.
    fn step(&self, request: StepRequest) -> StepResult {
        let state_hash = request.state_hash();
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return StepResult::RootNotFound,
            Err(err) => {
                return StepResult::Failure(StepError::TrackingCopy(TrackingCopyError::Storage(
                    err,
                )));
            }
        };
        let protocol_version = request.protocol_version();

        let seed = {
            // seeds address generator w/ era_end_timestamp_millis
            let mut bytes = match request.era_end_timestamp_millis().into_bytes() {
                Ok(bytes) => bytes,
                Err(bre) => {
                    return StepResult::Failure(StepError::TrackingCopy(
                        TrackingCopyError::BytesRepr(bre),
                    ));
                }
            };
            match &mut protocol_version.into_bytes() {
                Ok(next) => bytes.append(next),
                Err(bre) => {
                    return StepResult::Failure(StepError::TrackingCopy(
                        TrackingCopyError::BytesRepr(*bre),
                    ));
                }
            };
            match &mut request.next_era_id().into_bytes() {
                Ok(next) => bytes.append(next),
                Err(bre) => {
                    return StepResult::Failure(StepError::TrackingCopy(
                        TrackingCopyError::BytesRepr(*bre),
                    ));
                }
            };

            Id::Seed(bytes)
        };

        let config = request.config();
        // this runtime uses the system's context
        let phase = Phase::Session;
        let address_generator = AddressGenerator::new(&seed.seed(), phase);
        let mut runtime = match RuntimeNative::new_system_runtime(
            config.clone(),
            seed,
            Arc::new(RwLock::new(address_generator)),
            Rc::clone(&tc),
            phase,
        ) {
            Ok(rt) => rt,
            Err(tce) => return StepResult::Failure(StepError::TrackingCopy(tce)),
        };

        let slashed_validators: Vec<PublicKey> = request.slashed_validators();
        if !slashed_validators.is_empty() {
            if let Err(err) = runtime.slash(slashed_validators) {
                error!("{}", err);
                return StepResult::Failure(StepError::SlashingError);
            }
        }

        let era_end_timestamp_millis = request.era_end_timestamp_millis();
        let evicted_validators = request
            .evict_items()
            .iter()
            .map(|item| item.validator_id.clone())
            .collect::<Vec<PublicKey>>();
        let max_delegators_per_validator = config.max_delegators_per_validator();
        let include_credits = config.include_credits();
        let credit_cap = config.credit_cap();
        let minimum_bid_amount = config.minimum_bid_amount();

        if let Err(err) = runtime.run_auction(
            era_end_timestamp_millis,
            evicted_validators,
            max_delegators_per_validator,
            include_credits,
            credit_cap,
            minimum_bid_amount,
        ) {
            error!("{}", err);
            return StepResult::Failure(StepError::Auction);
        }

        let effects = tc.borrow().effects();

        match self.commit_effects(state_hash, effects.clone()) {
            Ok(post_state_hash) => StepResult::Success {
                post_state_hash,
                effects,
            },
            Err(gse) => StepResult::Failure(gse.into()),
        }
    }

    /// Distribute block rewards.
    fn distribute_block_rewards(&self, request: BlockRewardsRequest) -> BlockRewardsResult {
        let state_hash = request.state_hash();
        let rewards = request.rewards();
        if rewards.is_empty() {
            info!("rewards are empty");
            // if there are no rewards to distribute, this is effectively a noop
            return BlockRewardsResult::Success {
                post_state_hash: state_hash,
                effects: Effects::new(),
            };
        }

        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return BlockRewardsResult::RootNotFound,
            Err(err) => {
                return BlockRewardsResult::Failure(BlockRewardsError::TrackingCopy(
                    TrackingCopyError::Storage(err),
                ));
            }
        };

        let config = request.config();
        let protocol_version = request.protocol_version();
        let seed = {
            let mut bytes = match request.block_time().into_bytes() {
                Ok(bytes) => bytes,
                Err(bre) => {
                    return BlockRewardsResult::Failure(BlockRewardsError::TrackingCopy(
                        TrackingCopyError::BytesRepr(bre),
                    ));
                }
            };
            match &mut protocol_version.into_bytes() {
                Ok(next) => bytes.append(next),
                Err(bre) => {
                    return BlockRewardsResult::Failure(BlockRewardsError::TrackingCopy(
                        TrackingCopyError::BytesRepr(*bre),
                    ));
                }
            };

            Id::Seed(bytes)
        };

        // this runtime uses the system's context
        let phase = Phase::Session;
        let address_generator = AddressGenerator::new(&seed.seed(), phase);

        let mut runtime = match RuntimeNative::new_system_runtime(
            config.clone(),
            seed,
            Arc::new(RwLock::new(address_generator)),
            Rc::clone(&tc),
            phase,
        ) {
            Ok(rt) => rt,
            Err(tce) => {
                return BlockRewardsResult::Failure(BlockRewardsError::TrackingCopy(tce));
            }
        };

        if let Err(auction_error) = runtime.distribute(rewards.clone()) {
            error!(
                "distribute block rewards failed due to auction error {:?}",
                auction_error
            );
            return BlockRewardsResult::Failure(BlockRewardsError::Auction(auction_error));
        } else {
            debug!("rewards distribution complete");
        }

        let effects = tc.borrow().effects();

        match self.commit_effects(state_hash, effects.clone()) {
            Ok(post_state_hash) => {
                debug!("reward distribution committed");
                BlockRewardsResult::Success {
                    post_state_hash,
                    effects,
                }
            }
            Err(gse) => BlockRewardsResult::Failure(BlockRewardsError::TrackingCopy(
                TrackingCopyError::Storage(gse),
            )),
        }
    }

    /// Distribute fees, if relevant to the chainspec configured behavior.
    fn distribute_fees(&self, request: FeeRequest) -> FeeResult {
        let state_hash = request.state_hash();
        if !request.should_distribute_fees() {
            // effectively noop
            return FeeResult::Success {
                post_state_hash: state_hash,
                effects: Effects::new(),
                transfers: vec![],
            };
        }

        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
            Ok(None) => return FeeResult::RootNotFound,
            Err(gse) => {
                return FeeResult::Failure(FeeError::TrackingCopy(TrackingCopyError::Storage(gse)));
            }
        };

        let config = request.config();
        let protocol_version = request.protocol_version();
        let seed = {
            let mut bytes = match request.block_time().into_bytes() {
                Ok(bytes) => bytes,
                Err(bre) => {
                    return FeeResult::Failure(FeeError::TrackingCopy(
                        TrackingCopyError::BytesRepr(bre),
                    ));
                }
            };
            match &mut protocol_version.into_bytes() {
                Ok(next) => bytes.append(next),
                Err(bre) => {
                    return FeeResult::Failure(FeeError::TrackingCopy(
                        TrackingCopyError::BytesRepr(*bre),
                    ));
                }
            };

            Id::Seed(bytes)
        };

        // this runtime uses the system's context
        let phase = Phase::System;
        let address_generator = AddressGenerator::new(&seed.seed(), phase);
        let mut runtime = match RuntimeNative::new_system_runtime(
            config.clone(),
            seed,
            Arc::new(RwLock::new(address_generator)),
            Rc::clone(&tc),
            phase,
        ) {
            Ok(rt) => rt,
            Err(tce) => {
                return FeeResult::Failure(FeeError::TrackingCopy(tce));
            }
        };

        let source = BalanceIdentifier::Accumulate;
        let source_purse = match source.purse_uref(&mut tc.borrow_mut(), protocol_version) {
            Ok(value) => value,
            Err(tce) => return FeeResult::Failure(FeeError::TrackingCopy(tce)),
        };
        // amount = None will distribute the full current balance of the accumulation purse
        let result = runtime.distribute_accumulated_fees(source_purse, None);

        match result {
            Ok(_) => {
                let effects = tc.borrow_mut().effects();
                let transfers = runtime.into_transfers();
                let post_state_hash = match self.commit_effects(state_hash, effects.clone()) {
                    Ok(post_state_hash) => post_state_hash,
                    Err(gse) => {
                        return FeeResult::Failure(FeeError::TrackingCopy(
                            TrackingCopyError::Storage(gse),
                        ));
                    }
                };
                FeeResult::Success {
                    effects,
                    transfers,
                    post_state_hash,
                }
            }
            Err(hpe) => FeeResult::Failure(FeeError::TrackingCopy(
                TrackingCopyError::SystemContract(system::Error::HandlePayment(hpe)),
            )),
        }
    }

    /// Gets block global data.
    fn block_global(&self, request: BlockGlobalRequest) -> BlockGlobalResult {
        let state_hash = request.state_hash();
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
            Ok(None) => return BlockGlobalResult::RootNotFound,
            Err(gse) => return BlockGlobalResult::Failure(TrackingCopyError::Storage(gse)),
        };

        // match request
        match request.block_global_kind() {
            BlockGlobalKind::BlockTime(block_time) => {
                let cl_value =
                    match CLValue::from_t(block_time.value()).map_err(TrackingCopyError::CLValue) {
                        Ok(cl_value) => cl_value,
                        Err(tce) => {
                            return BlockGlobalResult::Failure(tce);
                        }
                    };
                tc.borrow_mut().write(
                    Key::BlockGlobal(BlockGlobalAddr::BlockTime),
                    StoredValue::CLValue(cl_value),
                );
            }
            BlockGlobalKind::MessageCount(count) => {
                let cl_value = match CLValue::from_t(count).map_err(TrackingCopyError::CLValue) {
                    Ok(cl_value) => cl_value,
                    Err(tce) => {
                        return BlockGlobalResult::Failure(tce);
                    }
                };
                tc.borrow_mut().write(
                    Key::BlockGlobal(BlockGlobalAddr::MessageCount),
                    StoredValue::CLValue(cl_value),
                );
            }
            BlockGlobalKind::ProtocolVersion(protocol_version) => {
                let cl_value = match CLValue::from_t(protocol_version.destructure())
                    .map_err(TrackingCopyError::CLValue)
                {
                    Ok(cl_value) => cl_value,
                    Err(tce) => {
                        return BlockGlobalResult::Failure(tce);
                    }
                };
                tc.borrow_mut().write(
                    Key::BlockGlobal(BlockGlobalAddr::ProtocolVersion),
                    StoredValue::CLValue(cl_value),
                );
            }
            BlockGlobalKind::AddressableEntity(addressable_entity) => {
                let cl_value =
                    match CLValue::from_t(addressable_entity).map_err(TrackingCopyError::CLValue) {
                        Ok(cl_value) => cl_value,
                        Err(tce) => {
                            return BlockGlobalResult::Failure(tce);
                        }
                    };
                tc.borrow_mut().write(
                    Key::BlockGlobal(BlockGlobalAddr::AddressableEntity),
                    StoredValue::CLValue(cl_value),
                );
            }
        }

        let effects = tc.borrow_mut().effects();

        let post_state_hash = match self.commit_effects(state_hash, effects.clone()) {
            Ok(post_state_hash) => post_state_hash,
            Err(gse) => return BlockGlobalResult::Failure(TrackingCopyError::Storage(gse)),
        };

        BlockGlobalResult::Success {
            post_state_hash,
            effects: Box::new(effects),
        }
    }
}

/// A trait expressing operations over the trie.
pub trait StateProvider: Send + Sync + Sized {
    /// Associated reader type for `StateProvider`.
    type Reader: StateReader<Key, StoredValue, Error = GlobalStateError>;

    /// Flush the state provider.
    fn flush(&self, request: FlushRequest) -> FlushResult;

    /// Returns an empty root hash.
    fn empty_root(&self) -> Digest;

    /// Get a tracking copy.
    fn tracking_copy(
        &self,
        state_hash: Digest,
    ) -> Result<Option<TrackingCopy<Self::Reader>>, GlobalStateError>;

    /// Checkouts a slice of initial state using root state hash.
    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, GlobalStateError>;

    /// Query state.
    fn query(&self, request: QueryRequest) -> QueryResult {
        match self.tracking_copy(request.state_hash()) {
            Ok(Some(tc)) => match tc.query(request.key(), request.path()) {
                Ok(ret) => ret.into(),
                Err(err) => QueryResult::Failure(err),
            },
            Ok(None) => QueryResult::RootNotFound,
            Err(err) => QueryResult::Failure(TrackingCopyError::Storage(err)),
        }
    }

    /// Message topics request.
    fn message_topics(&self, message_topics_request: MessageTopicsRequest) -> MessageTopicsResult {
        let tc = match self.tracking_copy(message_topics_request.state_hash()) {
            Ok(Some(tracking_copy)) => tracking_copy,
            Ok(None) => return MessageTopicsResult::RootNotFound,
            Err(err) => return MessageTopicsResult::Failure(err.into()),
        };

        match tc.get_message_topics(message_topics_request.entity_addr()) {
            Ok(message_topics) => MessageTopicsResult::Success { message_topics },
            Err(tce) => MessageTopicsResult::Failure(tce),
        }
    }

    /// Provides the underlying addr for the imputed balance identifier.
    fn balance_purse(
        &self,
        request: BalanceIdentifierPurseRequest,
    ) -> BalanceIdentifierPurseResult {
        let mut tc = match self.tracking_copy(request.state_hash()) {
            Ok(Some(tracking_copy)) => tracking_copy,
            Ok(None) => return BalanceIdentifierPurseResult::RootNotFound,
            Err(err) => return TrackingCopyError::Storage(err).into(),
        };
        let balance_identifier = request.identifier();
        let protocol_version = request.protocol_version();
        match balance_identifier.purse_uref(&mut tc, protocol_version) {
            Ok(uref) => BalanceIdentifierPurseResult::Success {
                purse_addr: uref.addr(),
            },
            Err(tce) => BalanceIdentifierPurseResult::Failure(tce),
        }
    }

    /// Balance inquiry.
    fn balance(&self, request: BalanceRequest) -> BalanceResult {
        let mut tc = match self.tracking_copy(request.state_hash()) {
            Ok(Some(tracking_copy)) => tracking_copy,
            Ok(None) => return BalanceResult::RootNotFound,
            Err(err) => return TrackingCopyError::Storage(err).into(),
        };
        let protocol_version = request.protocol_version();
        let balance_identifier = request.identifier();
        let purse_key = match balance_identifier.purse_uref(&mut tc, protocol_version) {
            Ok(value) => value.into(),
            Err(tce) => return tce.into(),
        };
        let (purse_balance_key, purse_addr) = match tc.get_purse_balance_key(purse_key) {
            Ok(key @ Key::Balance(addr)) => (key, addr),
            Ok(key) => return TrackingCopyError::UnexpectedKeyVariant(key).into(),
            Err(tce) => return tce.into(),
        };

        let (total_balance, proofs_result) = match request.proof_handling() {
            ProofHandling::NoProofs => {
                let total_balance = match tc.read(&purse_balance_key) {
                    Ok(Some(StoredValue::CLValue(cl_value))) => match cl_value.into_t::<U512>() {
                        Ok(val) => val,
                        Err(cve) => return TrackingCopyError::CLValue(cve).into(),
                    },
                    Ok(Some(_)) => return TrackingCopyError::UnexpectedStoredValueVariant.into(),
                    Ok(None) => return TrackingCopyError::KeyNotFound(purse_balance_key).into(),
                    Err(tce) => return tce.into(),
                };
                let balance_holds = match request.balance_handling() {
                    BalanceHandling::Total => BTreeMap::new(),
                    BalanceHandling::Available => {
                        match tc.get_balance_hold_config(BalanceHoldAddrTag::Gas) {
                            Ok(Some((block_time, _, interval))) => {
                                match tc.get_balance_holds(purse_addr, block_time, interval) {
                                    Ok(holds) => holds,
                                    Err(tce) => return tce.into(),
                                }
                            }
                            Ok(None) => BTreeMap::new(),
                            Err(tce) => return tce.into(),
                        }
                    }
                };
                (total_balance, ProofsResult::NotRequested { balance_holds })
            }
            ProofHandling::Proofs => {
                let (total_balance, total_balance_proof) =
                    match tc.get_total_balance_with_proof(purse_balance_key) {
                        Ok((balance, proof)) => (balance, Box::new(proof)),
                        Err(tce) => return tce.into(),
                    };

                let balance_holds = match request.balance_handling() {
                    BalanceHandling::Total => BTreeMap::new(),
                    BalanceHandling::Available => {
                        match tc.get_balance_holds_with_proof(purse_addr) {
                            Ok(holds) => holds,
                            Err(tce) => return tce.into(),
                        }
                    }
                };

                (
                    total_balance,
                    ProofsResult::Proofs {
                        total_balance_proof,
                        balance_holds,
                    },
                )
            }
        };

        let (block_time, gas_hold_handling) = match tc
            .get_balance_hold_config(BalanceHoldAddrTag::Gas)
        {
            Ok(Some((block_time, handling, interval))) => (block_time, (handling, interval).into()),
            Ok(None) => {
                return BalanceResult::Success {
                    purse_addr,
                    total_balance,
                    available_balance: total_balance,
                    proofs_result,
                };
            }
            Err(tce) => return tce.into(),
        };

        let processing_hold_handling =
            match tc.get_balance_hold_config(BalanceHoldAddrTag::Processing) {
                Ok(Some((_, handling, interval))) => (handling, interval).into(),
                Ok(None) => {
                    return BalanceResult::Success {
                        purse_addr,
                        total_balance,
                        available_balance: total_balance,
                        proofs_result,
                    };
                }
                Err(tce) => return tce.into(),
            };

        let available_balance = match &proofs_result.available_balance(
            block_time,
            total_balance,
            gas_hold_handling,
            processing_hold_handling,
        ) {
            Ok(available_balance) => *available_balance,
            Err(be) => return BalanceResult::Failure(TrackingCopyError::Balance(be.clone())),
        };

        BalanceResult::Success {
            purse_addr,
            total_balance,
            available_balance,
            proofs_result,
        }
    }

    /// Balance hold.
    fn balance_hold(&self, request: BalanceHoldRequest) -> BalanceHoldResult {
        let mut tc = match self.tracking_copy(request.state_hash()) {
            Ok(Some(tracking_copy)) => tracking_copy,
            Ok(None) => return BalanceHoldResult::RootNotFound,
            Err(err) => {
                return BalanceHoldResult::Failure(BalanceHoldError::TrackingCopy(
                    TrackingCopyError::Storage(err),
                ));
            }
        };
        let hold_mode = request.balance_hold_mode();
        match hold_mode {
            BalanceHoldMode::Hold {
                identifier,
                hold_amount,
                insufficient_handling,
            } => {
                let block_time = match tc.get_block_time() {
                    Ok(Some(block_time)) => block_time,
                    Ok(None) => return BalanceHoldResult::BlockTimeNotFound,
                    Err(tce) => return tce.into(),
                };
                let tag = match request.balance_hold_kind() {
                    BalanceHoldKind::All => {
                        return BalanceHoldResult::Failure(
                            BalanceHoldError::UnexpectedWildcardVariant,
                        );
                    }
                    BalanceHoldKind::Tag(tag) => tag,
                };
                let balance_request = BalanceRequest::new(
                    request.state_hash(),
                    request.protocol_version(),
                    identifier,
                    BalanceHandling::Available,
                    ProofHandling::NoProofs,
                );
                let balance_result = self.balance(balance_request);
                let (total_balance, remaining_balance, purse_addr) = match balance_result {
                    BalanceResult::RootNotFound => return BalanceHoldResult::RootNotFound,
                    BalanceResult::Failure(be) => return be.into(),
                    BalanceResult::Success {
                        total_balance,
                        available_balance,
                        purse_addr,
                        ..
                    } => (total_balance, available_balance, purse_addr),
                };

                let held_amount = {
                    if remaining_balance >= hold_amount {
                        // the purse has sufficient balance to fully cover the hold
                        hold_amount
                    } else if insufficient_handling == InsufficientBalanceHandling::Noop {
                        // the purse has insufficient balance and the insufficient
                        // balance handling mode is noop, so get out
                        return BalanceHoldResult::Failure(BalanceHoldError::InsufficientBalance {
                            remaining_balance,
                        });
                    } else {
                        // currently this is always the default HoldRemaining variant.
                        // the purse holder has insufficient balance to cover the hold,
                        // but the system will put a hold on whatever balance remains.
                        // this is basically punitive to block an edge case resource consumption
                        // attack whereby a malicious purse holder drains a balance to not-zero
                        // but not-enough-to-cover-holds and then spams a bunch of transactions
                        // knowing that they will fail due to insufficient funds, but only
                        // after making the system do the work of processing the balance
                        // check without penalty to themselves.
                        remaining_balance
                    }
                };

                let balance_hold_addr = match tag {
                    BalanceHoldAddrTag::Gas => BalanceHoldAddr::Gas {
                        purse_addr,
                        block_time,
                    },
                    BalanceHoldAddrTag::Processing => BalanceHoldAddr::Processing {
                        purse_addr,
                        block_time,
                    },
                };

                let hold_key = Key::BalanceHold(balance_hold_addr);
                let hold_value = match tc.get(&hold_key) {
                    Ok(Some(StoredValue::CLValue(cl_value))) => {
                        // There was a previous hold on this balance. We need to add the new hold to
                        // the old one.
                        match cl_value.clone().into_t::<U512>() {
                            Ok(prev_hold) => prev_hold.saturating_add(held_amount),
                            Err(cve) => {
                                return BalanceHoldResult::Failure(BalanceHoldError::TrackingCopy(
                                    TrackingCopyError::CLValue(cve),
                                ));
                            }
                        }
                    }
                    Ok(Some(other_value_variant)) => {
                        return BalanceHoldResult::Failure(BalanceHoldError::UnexpectedHoldValue(
                            other_value_variant,
                        ))
                    }
                    Ok(None) => held_amount, // There was no previous hold.
                    Err(tce) => {
                        return BalanceHoldResult::Failure(BalanceHoldError::TrackingCopy(tce));
                    }
                };

                let hold_cl_value = match CLValue::from_t(hold_value) {
                    Ok(cl_value) => cl_value,
                    Err(cve) => {
                        return BalanceHoldResult::Failure(BalanceHoldError::TrackingCopy(
                            TrackingCopyError::CLValue(cve),
                        ));
                    }
                };
                tc.write(hold_key, StoredValue::CLValue(hold_cl_value));
                let holds = vec![balance_hold_addr];

                let available_balance = remaining_balance.saturating_sub(held_amount);
                let effects = tc.effects();
                BalanceHoldResult::success(
                    Some(holds),
                    total_balance,
                    available_balance,
                    hold_amount,
                    held_amount,
                    effects,
                )
            }
            BalanceHoldMode::Clear { identifier } => {
                let purse_addr = match identifier.purse_uref(&mut tc, request.protocol_version()) {
                    Ok(source_purse) => source_purse.addr(),
                    Err(tce) => {
                        return BalanceHoldResult::Failure(BalanceHoldError::TrackingCopy(tce));
                    }
                };

                {
                    // clear holds
                    let hold_kind = request.balance_hold_kind();
                    let mut filter = vec![];
                    let tag = BalanceHoldAddrTag::Processing;
                    if hold_kind.matches(tag) {
                        let (block_time, interval) = match tc.get_balance_hold_config(tag) {
                            Ok(Some((block_time, _, interval))) => (block_time, interval),
                            Ok(None) => {
                                return BalanceHoldResult::BlockTimeNotFound;
                            }
                            Err(tce) => {
                                return BalanceHoldResult::Failure(BalanceHoldError::TrackingCopy(
                                    tce,
                                ));
                            }
                        };
                        filter.push((tag, HoldsEpoch::from_millis(block_time.value(), interval)));
                    }
                    let tag = BalanceHoldAddrTag::Gas;
                    if hold_kind.matches(tag) {
                        let (block_time, interval) = match tc.get_balance_hold_config(tag) {
                            Ok(Some((block_time, _, interval))) => (block_time, interval),
                            Ok(None) => {
                                return BalanceHoldResult::BlockTimeNotFound;
                            }
                            Err(tce) => {
                                return BalanceHoldResult::Failure(BalanceHoldError::TrackingCopy(
                                    tce,
                                ));
                            }
                        };
                        filter.push((tag, HoldsEpoch::from_millis(block_time.value(), interval)));
                    }
                    if let Err(tce) = tc.clear_expired_balance_holds(purse_addr, filter) {
                        return BalanceHoldResult::Failure(BalanceHoldError::TrackingCopy(tce));
                    }
                }

                // get updated balance
                let balance_result = self.balance(BalanceRequest::new(
                    request.state_hash(),
                    request.protocol_version(),
                    identifier,
                    BalanceHandling::Available,
                    ProofHandling::NoProofs,
                ));
                let (total_balance, available_balance) = match balance_result {
                    BalanceResult::RootNotFound => return BalanceHoldResult::RootNotFound,
                    BalanceResult::Failure(be) => return be.into(),
                    BalanceResult::Success {
                        total_balance,
                        available_balance,
                        ..
                    } => (total_balance, available_balance),
                };
                // note that hold & held in this context does not refer to remaining holds,
                // but rather to the requested hold amount and the resulting held amount for
                // this execution. as calls to this variant clears holds and does not create
                // new holds, hold & held are zero and no new hold address exists.
                let new_hold_addr = None;
                let hold = U512::zero();
                let held = U512::zero();
                let effects = tc.effects();
                BalanceHoldResult::success(
                    new_hold_addr,
                    total_balance,
                    available_balance,
                    hold,
                    held,
                    effects,
                )
            }
        }
    }

    /// Get the requested era validators.
    fn era_validators(&self, request: EraValidatorsRequest) -> EraValidatorsResult {
        match self.seigniorage_recipients(SeigniorageRecipientsRequest::new(request.state_hash())) {
            SeigniorageRecipientsResult::RootNotFound => EraValidatorsResult::RootNotFound,
            SeigniorageRecipientsResult::Failure(err) => EraValidatorsResult::Failure(err),
            SeigniorageRecipientsResult::ValueNotFound(msg) => {
                EraValidatorsResult::ValueNotFound(msg)
            }
            SeigniorageRecipientsResult::AuctionNotFound => EraValidatorsResult::AuctionNotFound,
            SeigniorageRecipientsResult::Success {
                seigniorage_recipients,
            } => {
                let era_validators = match seigniorage_recipients {
                    SeigniorageRecipientsSnapshot::V1(snapshot) => {
                        auction::detail::era_validators_from_legacy_snapshot(snapshot)
                    }
                    SeigniorageRecipientsSnapshot::V2(snapshot) => {
                        auction::detail::era_validators_from_snapshot(snapshot)
                    }
                };
                EraValidatorsResult::Success { era_validators }
            }
        }
    }

    /// Get the requested seigniorage recipients.
    fn seigniorage_recipients(
        &self,
        request: SeigniorageRecipientsRequest,
    ) -> SeigniorageRecipientsResult {
        let state_hash = request.state_hash();
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return SeigniorageRecipientsResult::RootNotFound,
            Err(err) => {
                return SeigniorageRecipientsResult::Failure(TrackingCopyError::Storage(err))
            }
        };
        let scr = match tc.get_system_entity_registry() {
            Ok(scr) => scr,
            Err(err) => return SeigniorageRecipientsResult::Failure(err),
        };
        let addressable_entity_enabled = tc.addressable_entity_enabled();
        match get_snapshot_data(self, &scr, state_hash, addressable_entity_enabled) {
            not_found @ SeigniorageRecipientsResult::ValueNotFound(_) => {
                if addressable_entity_enabled {
                    //There is a chance that, when looking for systemic data, we could be using a
                    // state root hash from before the AddressableEntity
                    // migration boundary. In such a case, we should attempt to look up the data
                    // under the Account/Contract model instead; e.g. Key::Hash instead of
                    // Key::AddressableEntity
                    match get_snapshot_data(self, &scr, state_hash, false) {
                        SeigniorageRecipientsResult::ValueNotFound(_) => not_found,
                        other => other,
                    }
                } else {
                    not_found
                }
            }
            other => other,
        }
    }

    /// Gets the bids.
    fn bids(&self, request: BidsRequest) -> BidsResult {
        let state_hash = request.state_hash();
        let mut tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return BidsResult::RootNotFound,
            Err(err) => return BidsResult::Failure(TrackingCopyError::Storage(err)),
        };

        let bid_keys = match tc.get_keys(&KeyTag::BidAddr) {
            Ok(ret) => ret,
            Err(err) => return BidsResult::Failure(err),
        };

        let mut bids = vec![];
        for key in bid_keys.iter() {
            match tc.get(key) {
                Ok(ret) => match ret {
                    Some(StoredValue::BidKind(bid_kind)) => {
                        if !bids.contains(&bid_kind) {
                            bids.push(bid_kind);
                        }
                    }
                    Some(_) => {
                        return BidsResult::Failure(
                            TrackingCopyError::UnexpectedStoredValueVariant,
                        );
                    }
                    None => return BidsResult::Failure(TrackingCopyError::MissingBid(*key)),
                },
                Err(error) => return BidsResult::Failure(error),
            }
        }
        BidsResult::Success { bids }
    }

    /// Fetches the validator bids.
    /// This will return either:
    /// * All BidKind state entries which were stored under a key relevant to the validator
    /// * All Bids wrapped in BidKind::Unified which keys match [KeyTag::Bid,
    ///   account_hash_of_public_key_bytes] if no bids were found in step 1
    fn validator_bids(&self, request: ValidatorBidRequest) -> ValidatorBidsResult {
        let state_hash = request.state_root_hash();
        let mut tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return ValidatorBidsResult::RootNotFound,
            Err(err) => return ValidatorBidsResult::Failure(TrackingCopyError::Storage(err)),
        };
        let account_hash = request.validator_key().to_account_hash();

        let result = find_contemporary_validator_bids(&mut tc, &account_hash);
        match result {
            ValidatorBidsResult::RootNotFound => ValidatorBidsResult::RootNotFound,
            ValidatorBidsResult::Success { bids } => {
                if bids.is_empty() {
                    find_historic_validator_bids(&mut tc, &account_hash)
                } else {
                    ValidatorBidsResult::Success { bids }
                }
            }
            ValidatorBidsResult::Failure(error) => ValidatorBidsResult::Failure(error),
        }
    }

    /// Fetches the bids relevant to a delegator in scope of a validator.
    /// This will return either:
    /// * All BidKind state entries which were stored under a key relevant to the delegator in scope
    ///   of a validator
    /// * If step 1 yielded no data - we will attempt to retrofit 1.x data into the new schema by:
    ///     * if the request delegator is not of DelegatorKind::PublicKey variant - return empty
    ///     * fetch the Bid entry relevant to the given delegator
    ///     * find the Bid delegators map entry relevant to the public key of the given delegator
    ///     * remap the [`DelegatorBidRequest`] structure to BidKind::Delegator(delegator)
    fn delegator_bids(&self, request: DelegatorBidRequest) -> DelegatorBidsResult {
        let state_hash = request.state_root_hash();
        let mut tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return DelegatorBidsResult::RootNotFound,
            Err(err) => return DelegatorBidsResult::Failure(TrackingCopyError::Storage(err)),
        };
        let validator_public_key = request.validator_key();
        let account_hash = validator_public_key.to_account_hash();
        let delegator_kind = request.delegator();
        match find_contemporary_delegator_bids(&mut tc, &account_hash, delegator_kind) {
            Ok(bids) => {
                if bids.is_empty() {
                    match delegator_kind {
                        DelegatorKind::PublicKey(delegator_public_key) => {
                            match find_historic_delegator_bids(
                                &mut tc,
                                validator_public_key,
                                delegator_public_key,
                            ) {
                                Ok(bids) => DelegatorBidsResult::Success { bids },
                                Err(err) => DelegatorBidsResult::Failure(err),
                            }
                        }
                        DelegatorKind::Purse(_) => {
                            /* Cannot retrofit purse */
                            DelegatorBidsResult::Success { bids: vec![] }
                        }
                    }
                } else {
                    DelegatorBidsResult::Success { bids }
                }
            }
            Err(err) => DelegatorBidsResult::Failure(err),
        }
    }

    /// Direct auction interaction for all variations of bid management.
    fn bidding(
        &self,
        BiddingRequest {
            config,
            state_hash,
            auction_method,
            transaction_hash,
            initiator,
            authorization_keys,
        }: BiddingRequest,
    ) -> BiddingResult {
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return BiddingResult::RootNotFound,
            Err(err) => return BiddingResult::Failure(TrackingCopyError::Storage(err)),
        };

        let source_account_hash = initiator.account_hash();
        let (entity_addr, mut footprint, mut entity_access_rights) = match tc
            .borrow_mut()
            .authorized_runtime_footprint_with_access_rights(
                config.protocol_version(),
                source_account_hash,
                &authorization_keys,
                &BTreeSet::default(),
            ) {
            Ok(ret) => ret,
            Err(tce) => {
                return BiddingResult::Failure(tce);
            }
        };
        let entity_key = Key::AddressableEntity(entity_addr);

        // extend named keys with era end timestamp
        match tc
            .borrow_mut()
            .system_contract_named_key(AUCTION, ERA_END_TIMESTAMP_MILLIS_KEY)
        {
            Ok(Some(k)) => {
                match k.as_uref() {
                    Some(uref) => entity_access_rights.extend(&[*uref]),
                    None => {
                        return BiddingResult::Failure(TrackingCopyError::UnexpectedKeyVariant(k));
                    }
                }
                footprint.insert_into_named_keys(ERA_END_TIMESTAMP_MILLIS_KEY.into(), k);
            }
            Ok(None) => {
                return BiddingResult::Failure(TrackingCopyError::NamedKeyNotFound(
                    ERA_END_TIMESTAMP_MILLIS_KEY.into(),
                ));
            }
            Err(tce) => {
                return BiddingResult::Failure(tce);
            }
        };
        // extend named keys with era id
        match tc
            .borrow_mut()
            .system_contract_named_key(AUCTION, ERA_ID_KEY)
        {
            Ok(Some(k)) => {
                match k.as_uref() {
                    Some(uref) => entity_access_rights.extend(&[*uref]),
                    None => {
                        return BiddingResult::Failure(TrackingCopyError::UnexpectedKeyVariant(k));
                    }
                }
                footprint.insert_into_named_keys(ERA_ID_KEY.into(), k);
            }
            Ok(None) => {
                return BiddingResult::Failure(TrackingCopyError::NamedKeyNotFound(
                    ERA_ID_KEY.into(),
                ));
            }
            Err(tce) => {
                return BiddingResult::Failure(tce);
            }
        };

        let phase = Phase::Session;
        let id = Id::Transaction(transaction_hash);
        let address_generator = AddressGenerator::new(&id.seed(), phase);
        let max_delegators_per_validator = config.max_delegators_per_validator();
        let minimum_bid_amount = config.minimum_bid_amount();
        let mut runtime = RuntimeNative::new(
            config,
            id,
            Arc::new(RwLock::new(address_generator)),
            Rc::clone(&tc),
            source_account_hash,
            entity_key,
            footprint,
            entity_access_rights,
            U512::MAX,
            phase,
        );

        let result = match auction_method {
            AuctionMethod::ActivateBid { validator } => runtime
                .activate_bid(validator, minimum_bid_amount)
                .map(|_| AuctionMethodRet::Unit)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
            AuctionMethod::AddBid {
                public_key,
                delegation_rate,
                amount,
                vesting_schedule_period_millis,
                minimum_delegation_amount,
                maximum_delegation_amount,
                minimum_bid_amount,
                reserved_slots,
            } => runtime
                .add_bid(
                    public_key,
                    delegation_rate,
                    amount,
                    vesting_schedule_period_millis,
                    minimum_delegation_amount,
                    maximum_delegation_amount,
                    minimum_bid_amount,
                    max_delegators_per_validator,
                    reserved_slots,
                )
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(TrackingCopyError::Api),
            AuctionMethod::WithdrawBid {
                public_key,
                amount,
                minimum_bid_amount,
            } => runtime
                .withdraw_bid(public_key, amount, minimum_bid_amount)
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
            AuctionMethod::Delegate {
                delegator,
                validator,
                amount,
                max_delegators_per_validator,
            } => runtime
                .delegate(delegator, validator, amount, max_delegators_per_validator)
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(TrackingCopyError::Api),
            AuctionMethod::Undelegate {
                delegator,
                validator,
                amount,
            } => runtime
                .undelegate(delegator, validator, amount)
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
            AuctionMethod::Redelegate {
                delegator,
                validator,
                amount,
                new_validator,
            } => runtime
                .redelegate(delegator, validator, amount, new_validator)
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
            AuctionMethod::ChangeBidPublicKey {
                public_key,
                new_public_key,
            } => runtime
                .change_bid_public_key(public_key, new_public_key)
                .map(|_| AuctionMethodRet::Unit)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
            AuctionMethod::AddReservations { reservations } => runtime
                .add_reservations(reservations)
                .map(|_| AuctionMethodRet::Unit)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
            AuctionMethod::CancelReservations {
                validator,
                delegators,
                max_delegators_per_validator,
            } => runtime
                .cancel_reservations(validator, delegators, max_delegators_per_validator)
                .map(|_| AuctionMethodRet::Unit)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
        };

        let transfers = runtime.into_transfers();
        let effects = tc.borrow_mut().effects();

        match result {
            Ok(ret) => BiddingResult::Success {
                ret,
                effects,
                transfers,
            },
            Err(tce) => BiddingResult::Failure(tce),
        }
    }

    /// Handle refund.
    fn handle_refund(
        &self,
        HandleRefundRequest {
            config,
            state_hash,
            protocol_version,
            transaction_hash,
            refund_mode,
        }: HandleRefundRequest,
    ) -> HandleRefundResult {
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return HandleRefundResult::RootNotFound,
            Err(err) => return HandleRefundResult::Failure(TrackingCopyError::Storage(err)),
        };

        let id = Id::Transaction(transaction_hash);
        let phase = refund_mode.phase();
        let address_generator = Arc::new(RwLock::new(AddressGenerator::new(&id.seed(), phase)));
        let mut runtime = match phase {
            Phase::FinalizePayment => {
                // this runtime uses the system's context
                match RuntimeNative::new_system_runtime(
                    config,
                    id,
                    address_generator,
                    Rc::clone(&tc),
                    phase,
                ) {
                    Ok(rt) => rt,
                    Err(tce) => {
                        return HandleRefundResult::Failure(tce);
                    }
                }
            }
            Phase::Payment => {
                // this runtime uses the handle payment contract's context
                match RuntimeNative::new_system_contract_runtime(
                    config,
                    id,
                    address_generator,
                    Rc::clone(&tc),
                    phase,
                    HANDLE_PAYMENT,
                ) {
                    Ok(rt) => rt,
                    Err(tce) => {
                        return HandleRefundResult::Failure(tce);
                    }
                }
            }
            Phase::System | Phase::Session => return HandleRefundResult::InvalidPhase,
        };

        let result = match refund_mode {
            HandleRefundMode::CalculateAmount {
                limit,
                cost,
                gas_price,
                consumed,
                ratio,
                available,
            } => {
                let (numer, denom) = ratio.into();
                let ratio = Ratio::new_raw(U512::from(numer), U512::from(denom));
                let refund_amount = match runtime.calculate_overpayment_and_fee(
                    limit, gas_price, cost, consumed, ratio, available,
                ) {
                    Ok((refund, _)) => Some(refund),
                    Err(hpe) => {
                        return HandleRefundResult::Failure(TrackingCopyError::SystemContract(
                            system::Error::HandlePayment(hpe),
                        ));
                    }
                };
                Ok(refund_amount)
            }
            HandleRefundMode::Refund {
                initiator_addr,
                limit,
                cost,
                gas_price,
                consumed,
                ratio,
                source,
                target,
                available,
            } => {
                let source_purse = match source.purse_uref(&mut tc.borrow_mut(), protocol_version) {
                    Ok(value) => value,
                    Err(tce) => return HandleRefundResult::Failure(tce),
                };
                let (numer, denom) = ratio.into();
                let ratio = Ratio::new_raw(U512::from(numer), U512::from(denom));
                let refund_amount = match runtime.calculate_overpayment_and_fee(
                    limit, gas_price, cost, consumed, ratio, available,
                ) {
                    Ok((refund, _)) => refund,
                    Err(hpe) => {
                        return HandleRefundResult::Failure(TrackingCopyError::SystemContract(
                            system::Error::HandlePayment(hpe),
                        ));
                    }
                };
                let target_purse = match target.purse_uref(&mut tc.borrow_mut(), protocol_version) {
                    Ok(value) => value,
                    Err(tce) => return HandleRefundResult::Failure(tce),
                };
                // pay amount from source to target
                match runtime
                    .transfer(
                        Some(initiator_addr.account_hash()),
                        source_purse,
                        target_purse,
                        refund_amount,
                        None,
                    )
                    .map_err(|mint_err| {
                        TrackingCopyError::SystemContract(system::Error::Mint(mint_err))
                    }) {
                    Ok(_) => Ok(Some(refund_amount)),
                    Err(err) => Err(err),
                }
            }
            HandleRefundMode::RefundNoFeeCustomPayment {
                initiator_addr,
                limit,
                cost,
                gas_price,
            } => {
                let balance_result = self.balance(BalanceRequest::new(
                    state_hash,
                    protocol_version,
                    BalanceIdentifier::Payment,
                    BalanceHandling::Available,
                    ProofHandling::NoProofs,
                ));
                let available_balance = match balance_result {
                    BalanceResult::RootNotFound => {
                        return HandleRefundResult::RootNotFound;
                    }
                    BalanceResult::Failure(tce) => {
                        return HandleRefundResult::Failure(tce);
                    }
                    BalanceResult::Success {
                        available_balance, ..
                    } => available_balance,
                };

                let consumed = U512::zero();
                let ratio = Ratio::new_raw(U512::one(), U512::one());

                let refund_amount = match runtime.calculate_overpayment_and_fee(
                    limit,
                    gas_price,
                    cost,
                    consumed,
                    ratio,
                    available_balance,
                ) {
                    Ok((refund, _)) => refund,
                    Err(hpe) => {
                        return HandleRefundResult::Failure(TrackingCopyError::SystemContract(
                            system::Error::HandlePayment(hpe),
                        ));
                    }
                };
                let source_purse = match BalanceIdentifier::Payment
                    .purse_uref(&mut tc.borrow_mut(), protocol_version)
                {
                    Ok(value) => value,
                    Err(tce) => return HandleRefundResult::Failure(tce),
                };
                let target_purse = match BalanceIdentifier::Refund
                    .purse_uref(&mut tc.borrow_mut(), protocol_version)
                {
                    Ok(value) => value,
                    Err(tce) => return HandleRefundResult::Failure(tce),
                };
                match runtime
                    .transfer(
                        Some(initiator_addr.account_hash()),
                        source_purse,
                        target_purse,
                        refund_amount,
                        None,
                    )
                    .map_err(|mint_err| {
                        TrackingCopyError::SystemContract(system::Error::Mint(mint_err))
                    }) {
                    Ok(_) => Ok(Some(U512::zero())), // return 0 in this mode
                    Err(err) => Err(err),
                }
            }
            HandleRefundMode::Burn {
                limit,
                gas_price,
                cost,
                consumed,
                source,
                ratio,
                available,
            } => {
                let source_purse = match source.purse_uref(&mut tc.borrow_mut(), protocol_version) {
                    Ok(value) => value,
                    Err(tce) => return HandleRefundResult::Failure(tce),
                };
                let (numer, denom) = ratio.into();
                let ratio = Ratio::new_raw(U512::from(numer), U512::from(denom));
                let burn_amount = match runtime.calculate_overpayment_and_fee(
                    limit, gas_price, cost, consumed, ratio, available,
                ) {
                    Ok((amount, _)) => Some(amount),
                    Err(hpe) => {
                        return HandleRefundResult::Failure(TrackingCopyError::SystemContract(
                            system::Error::HandlePayment(hpe),
                        ));
                    }
                };
                match runtime.payment_burn(source_purse, burn_amount) {
                    Ok(_) => Ok(burn_amount),
                    Err(hpe) => Err(TrackingCopyError::SystemContract(
                        system::Error::HandlePayment(hpe),
                    )),
                }
            }
            HandleRefundMode::SetRefundPurse { target } => {
                let target_purse = match target.purse_uref(&mut tc.borrow_mut(), protocol_version) {
                    Ok(value) => value,
                    Err(tce) => return HandleRefundResult::Failure(tce),
                };
                match runtime.set_refund_purse(target_purse) {
                    Ok(_) => Ok(None),
                    Err(hpe) => Err(TrackingCopyError::SystemContract(
                        system::Error::HandlePayment(hpe),
                    )),
                }
            }
            HandleRefundMode::ClearRefundPurse => match runtime.clear_refund_purse() {
                Ok(_) => Ok(None),
                Err(hpe) => Err(TrackingCopyError::SystemContract(
                    system::Error::HandlePayment(hpe),
                )),
            },
        };

        let effects = tc.borrow_mut().effects();
        let transfers = runtime.into_transfers();

        match result {
            Ok(amount) => HandleRefundResult::Success {
                transfers,
                effects,
                amount,
            },
            Err(tce) => HandleRefundResult::Failure(tce),
        }
    }

    /// Handle payment.
    fn handle_fee(
        &self,
        HandleFeeRequest {
            config,
            state_hash,
            transaction_hash,
            handle_fee_mode,
        }: HandleFeeRequest,
    ) -> HandleFeeResult {
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return HandleFeeResult::RootNotFound,
            Err(err) => return HandleFeeResult::Failure(TrackingCopyError::Storage(err)),
        };

        // this runtime uses the system's context

        let id = Id::Transaction(transaction_hash);
        let phase = Phase::FinalizePayment;
        let address_generator = AddressGenerator::new(&id.seed(), phase);
        let protocol_version = config.protocol_version();
        let mut runtime = match RuntimeNative::new_system_runtime(
            config,
            id,
            Arc::new(RwLock::new(address_generator)),
            Rc::clone(&tc),
            phase,
        ) {
            Ok(rt) => rt,
            Err(tce) => {
                return HandleFeeResult::Failure(tce);
            }
        };

        if let Some(source) = handle_fee_mode.maybe_source() {
            let source_purse = match source.purse_uref(&mut tc.borrow_mut(), protocol_version) {
                Ok(source_purse) => source_purse,
                Err(tce) => return HandleFeeResult::Failure(tce),
            };
            runtime.extend_access_rights(&[source_purse]);
        }

        let result = match handle_fee_mode {
            HandleFeeMode::Credit {
                validator,
                amount,
                era_id,
            } => runtime
                .write_validator_credit(*validator, era_id, amount)
                .map(|_| ())
                .map_err(|auction_error| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auction_error))
                }),
            HandleFeeMode::Pay {
                initiator_addr,
                amount,
                source,
                target,
            } => {
                let protocol_version = runtime.protocol_version();
                let source_purse = match source.purse_uref(&mut tc.borrow_mut(), protocol_version) {
                    Ok(value) => value,
                    Err(tce) => return HandleFeeResult::Failure(tce),
                };
                let target_purse = match target.purse_uref(&mut tc.borrow_mut(), protocol_version) {
                    Ok(value) => value,
                    Err(tce) => return HandleFeeResult::Failure(tce),
                };
                runtime
                    .transfer(
                        Some(initiator_addr.account_hash()),
                        source_purse,
                        target_purse,
                        amount,
                        None,
                    )
                    .map_err(|mint_err| {
                        TrackingCopyError::SystemContract(system::Error::Mint(mint_err))
                    })
            }
            HandleFeeMode::Burn { source, amount } => {
                let protocol_version = runtime.protocol_version();
                let source_purse = match source.purse_uref(&mut tc.borrow_mut(), protocol_version) {
                    Ok(value) => value,
                    Err(tce) => return HandleFeeResult::Failure(tce),
                };
                runtime
                    .payment_burn(source_purse, amount)
                    .map_err(|handle_payment_error| {
                        TrackingCopyError::SystemContract(system::Error::HandlePayment(
                            handle_payment_error,
                        ))
                    })
            }
        };

        let effects = tc.borrow_mut().effects();
        let transfers = runtime.into_transfers();

        match result {
            Ok(_) => HandleFeeResult::Success { transfers, effects },
            Err(tce) => HandleFeeResult::Failure(tce),
        }
    }

    /// Gets the execution result checksum.
    fn execution_result_checksum(
        &self,
        request: ExecutionResultsChecksumRequest,
    ) -> ExecutionResultsChecksumResult {
        let state_hash = request.state_hash();
        let mut tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return ExecutionResultsChecksumResult::RootNotFound,
            Err(err) => {
                return ExecutionResultsChecksumResult::Failure(TrackingCopyError::Storage(err));
            }
        };
        match tc.get_checksum_registry() {
            Ok(Some(registry)) => match registry.get(EXECUTION_RESULTS_CHECKSUM_NAME) {
                Some(checksum) => ExecutionResultsChecksumResult::Success {
                    checksum: *checksum,
                },
                None => ExecutionResultsChecksumResult::ChecksumNotFound,
            },
            Ok(None) => ExecutionResultsChecksumResult::RegistryNotFound,
            Err(err) => ExecutionResultsChecksumResult::Failure(err),
        }
    }

    /// Gets an addressable entity.
    fn addressable_entity(&self, request: AddressableEntityRequest) -> AddressableEntityResult {
        let key = request.key();
        let query_key = match key {
            Key::Account(_) => {
                let query_request = QueryRequest::new(request.state_hash(), key, vec![]);
                match self.query(query_request) {
                    QueryResult::RootNotFound => return AddressableEntityResult::RootNotFound,
                    QueryResult::ValueNotFound(msg) => {
                        return AddressableEntityResult::ValueNotFound(msg);
                    }
                    QueryResult::Failure(err) => return AddressableEntityResult::Failure(err),
                    QueryResult::Success { value, .. } => {
                        if let StoredValue::Account(account) = *value {
                            // legacy account that has not been migrated
                            let entity = AddressableEntity::from(account);
                            return AddressableEntityResult::Success { entity };
                        }
                        if let StoredValue::CLValue(cl_value) = &*value {
                            // the corresponding entity key should be under the account's key
                            match cl_value.clone().into_t::<Key>() {
                                Ok(entity_key @ Key::AddressableEntity(_)) => entity_key,
                                Ok(invalid_key) => {
                                    warn!(
                                        %key,
                                        %invalid_key,
                                        type_name = %value.type_name(),
                                        "expected a Key::AddressableEntity to be stored under account hash"
                                    );
                                    return AddressableEntityResult::Failure(
                                        TrackingCopyError::UnexpectedStoredValueVariant,
                                    );
                                }
                                Err(error) => {
                                    error!(%key, %error, "expected a CLValue::Key to be stored under account hash");
                                    return AddressableEntityResult::Failure(
                                        TrackingCopyError::CLValue(error),
                                    );
                                }
                            }
                        } else {
                            warn!(
                                %key,
                                type_name = %value.type_name(),
                                "expected a CLValue::Key or Account to be stored under account hash"
                            );
                            return AddressableEntityResult::Failure(
                                TrackingCopyError::UnexpectedStoredValueVariant,
                            );
                        }
                    }
                }
            }
            Key::Hash(contract_hash) => {
                let query_request = QueryRequest::new(request.state_hash(), key, vec![]);
                match self.query(query_request) {
                    QueryResult::RootNotFound => return AddressableEntityResult::RootNotFound,
                    QueryResult::ValueNotFound(msg) => {
                        return AddressableEntityResult::ValueNotFound(msg);
                    }
                    QueryResult::Failure(err) => return AddressableEntityResult::Failure(err),
                    QueryResult::Success { value, .. } => {
                        if let StoredValue::Contract(contract) = *value {
                            // legacy contract that has not been migrated
                            let entity = AddressableEntity::from(contract);
                            return AddressableEntityResult::Success { entity };
                        }
                        Key::AddressableEntity(EntityAddr::SmartContract(contract_hash))
                    }
                }
            }
            Key::AddressableEntity(_) => key,
            _ => {
                return AddressableEntityResult::Failure(TrackingCopyError::UnexpectedKeyVariant(
                    key,
                ));
            }
        };

        let query_request = QueryRequest::new(request.state_hash(), query_key, vec![]);
        match self.query(query_request) {
            QueryResult::RootNotFound => AddressableEntityResult::RootNotFound,
            QueryResult::ValueNotFound(msg) => AddressableEntityResult::ValueNotFound(msg),
            QueryResult::Success { value, .. } => {
                let entity = match value.as_addressable_entity() {
                    Some(entity) => entity.clone(),
                    None => {
                        return AddressableEntityResult::Failure(
                            TrackingCopyError::UnexpectedStoredValueVariant,
                        );
                    }
                };
                AddressableEntityResult::Success { entity }
            }
            QueryResult::Failure(err) => AddressableEntityResult::Failure(err),
        }
    }

    /// Returns the system entity registry or the key for a system entity registered within it.
    fn system_entity_registry(
        &self,
        request: SystemEntityRegistryRequest,
    ) -> SystemEntityRegistryResult {
        let state_hash = request.state_hash();
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return SystemEntityRegistryResult::RootNotFound,
            Err(err) => {
                return SystemEntityRegistryResult::Failure(TrackingCopyError::Storage(err));
            }
        };

        let reg = match tc.get_system_entity_registry() {
            Ok(reg) => reg,
            Err(tce) => {
                return SystemEntityRegistryResult::Failure(tce);
            }
        };

        let selector = request.selector();
        match selector {
            SystemEntityRegistrySelector::All => SystemEntityRegistryResult::Success {
                selected: selector.clone(),
                payload: SystemEntityRegistryPayload::All(reg),
            },
            SystemEntityRegistrySelector::ByName(name) => match reg.get(name).copied() {
                Some(entity_hash) => {
                    let key = if !request.addressable_entity_enabled() {
                        Key::Hash(entity_hash)
                    } else {
                        Key::AddressableEntity(EntityAddr::System(entity_hash))
                    };
                    SystemEntityRegistryResult::Success {
                        selected: selector.clone(),
                        payload: SystemEntityRegistryPayload::EntityKey(key),
                    }
                }
                None => {
                    error!("unexpected query failure; mint not found");
                    SystemEntityRegistryResult::NamedEntityNotFound(name.clone())
                }
            },
        }
    }

    /// Gets an entry point value.
    fn entry_point(&self, request: EntryPointRequest) -> EntryPointResult {
        let state_root_hash = request.state_hash();
        let contract_hash = request.contract_hash();
        let entry_point_name = request.entry_point_name();
        match EntryPointAddr::new_v1_entry_point_addr(
            EntityAddr::SmartContract(contract_hash),
            entry_point_name,
        ) {
            Ok(entry_point_addr) => {
                let key = Key::EntryPoint(entry_point_addr);
                let query_request = QueryRequest::new(request.state_hash(), key, vec![]);
                //We first check if the entry point exists as a stand alone 2.x entity
                match self.query(query_request) {
                    QueryResult::RootNotFound => EntryPointResult::RootNotFound,
                    QueryResult::ValueNotFound(query_result_not_found_msg) => {
                        //If the entry point was not found as a 2.x entity, we check if it exists
                        // as part of a 1.x contract
                        let contract_key = Key::Hash(contract_hash);
                        let contract_request = ContractRequest::new(state_root_hash, contract_key);
                        match self.contract(contract_request) {
                            ContractResult::Failure(tce) => EntryPointResult::Failure(tce),
                            ContractResult::ValueNotFound(_) => {
                                EntryPointResult::ValueNotFound(query_result_not_found_msg)
                            }
                            ContractResult::RootNotFound => EntryPointResult::RootNotFound,
                            ContractResult::Success { contract } => {
                                match contract.entry_points().get(entry_point_name) {
                                    Some(contract_entry_point) => EntryPointResult::Success {
                                        entry_point: EntryPointValue::V1CasperVm(
                                            EntityEntryPoint::from(contract_entry_point),
                                        ),
                                    },
                                    None => {
                                        EntryPointResult::ValueNotFound(query_result_not_found_msg)
                                    }
                                }
                            }
                        }
                    }
                    QueryResult::Failure(tce) => EntryPointResult::Failure(tce),
                    QueryResult::Success { value, .. } => {
                        if let StoredValue::EntryPoint(entry_point) = *value {
                            EntryPointResult::Success { entry_point }
                        } else {
                            error!("Expected to get entry point value received other variant");
                            EntryPointResult::Failure(
                                TrackingCopyError::UnexpectedStoredValueVariant,
                            )
                        }
                    }
                }
            }
            Err(_) => EntryPointResult::Failure(
                //TODO maybe we can have a better error type here
                TrackingCopyError::ValueNotFound("Entry point not found".to_string()),
            ),
        }
    }

    /// Gets a contract value.
    fn contract(&self, request: ContractRequest) -> ContractResult {
        let query_request = QueryRequest::new(request.state_hash(), request.key(), vec![]);

        match self.query(query_request) {
            QueryResult::RootNotFound => ContractResult::RootNotFound,
            QueryResult::ValueNotFound(msg) => ContractResult::ValueNotFound(msg),
            QueryResult::Failure(tce) => ContractResult::Failure(tce),
            QueryResult::Success { value, .. } => {
                if let StoredValue::Contract(contract) = *value {
                    ContractResult::Success { contract }
                } else {
                    error!("Expected to get contract value received other variant");
                    ContractResult::Failure(TrackingCopyError::UnexpectedStoredValueVariant)
                }
            }
        }
    }

    /// Gets an entry point value.
    fn entry_point_exists(&self, request: EntryPointExistsRequest) -> EntryPointExistsResult {
        match self.entry_point(request.into()) {
            EntryPointResult::RootNotFound => EntryPointExistsResult::RootNotFound,
            EntryPointResult::ValueNotFound(msg) => EntryPointExistsResult::ValueNotFound(msg),
            EntryPointResult::Success { .. } => EntryPointExistsResult::Success,
            EntryPointResult::Failure(error) => EntryPointExistsResult::Failure(error),
        }
    }

    /// Gets total supply.
    fn total_supply(&self, request: TotalSupplyRequest) -> TotalSupplyResult {
        let state_hash = request.state_hash();
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return TotalSupplyResult::RootNotFound,
            Err(err) => return TotalSupplyResult::Failure(TrackingCopyError::Storage(err)),
        };
        let scr = match tc.get_system_entity_registry() {
            Ok(scr) => scr,
            Err(err) => return TotalSupplyResult::Failure(err),
        };
        let addressable_entity_enabled = tc.addressable_entity_enabled();
        match get_total_supply_data(self, &scr, state_hash, addressable_entity_enabled) {
            not_found @ TotalSupplyResult::ValueNotFound(_) => {
                if addressable_entity_enabled {
                    //There is a chance that, when looking for systemic data, we could be using a
                    // state root hash from before the AddressableEntity
                    // migration boundary. In such a case, we should attempt to look up the data
                    // under the Account/Contract model instead; e.g. Key::Hash instead of
                    // Key::AddressableEntity
                    match get_total_supply_data(self, &scr, state_hash, false) {
                        TotalSupplyResult::ValueNotFound(_) => not_found,
                        other => other,
                    }
                } else {
                    not_found
                }
            }
            other => other,
        }
    }

    /// Gets the current round seigniorage rate.
    fn round_seigniorage_rate(
        &self,
        request: RoundSeigniorageRateRequest,
    ) -> RoundSeigniorageRateResult {
        let state_hash = request.state_hash();
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return RoundSeigniorageRateResult::RootNotFound,
            Err(err) => {
                return RoundSeigniorageRateResult::Failure(TrackingCopyError::Storage(err));
            }
        };
        let scr = match tc.get_system_entity_registry() {
            Ok(scr) => scr,
            Err(err) => return RoundSeigniorageRateResult::Failure(err),
        };
        let addressable_entity_enabled = tc.addressable_entity_enabled();
        match get_round_seigniorage_rate_data(self, &scr, state_hash, addressable_entity_enabled) {
            not_found @ RoundSeigniorageRateResult::ValueNotFound(_) => {
                if addressable_entity_enabled {
                    //There is a chance that, when looking for systemic data, we could be using a
                    // state root hash from before the AddressableEntity
                    // migration boundary. In such a case, we should attempt to look up the data
                    // under the Account/Contract model instead; e.g. Key::Hash instead of
                    // Key::AddressableEntity
                    match get_round_seigniorage_rate_data(self, &scr, state_hash, false) {
                        RoundSeigniorageRateResult::ValueNotFound(_) => not_found,
                        other => other,
                    }
                } else {
                    not_found
                }
            }
            other => other,
        }
    }

    /// Direct transfer.
    fn transfer(&self, request: TransferRequest) -> TransferResult {
        let state_hash = request.state_hash();
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return TransferResult::RootNotFound,
            Err(err) => {
                return TransferResult::Failure(TransferError::TrackingCopy(
                    TrackingCopyError::Storage(err),
                ));
            }
        };

        let source_account_hash = request.initiator().account_hash();
        let protocol_version = request.protocol_version();
        if let Err(tce) = tc
            .borrow_mut()
            .migrate_account(source_account_hash, protocol_version)
        {
            return TransferResult::Failure(tce.into());
        }

        let authorization_keys = request.authorization_keys();

        let config = request.config();
        let transfer_config = config.transfer_config();
        let administrative_accounts = transfer_config.administrative_accounts();

        let runtime_args = match request.args() {
            TransferRequestArgs::Raw(runtime_args) => runtime_args.clone(),
            TransferRequestArgs::Explicit(transfer_args) => {
                match RuntimeArgs::try_from(*transfer_args) {
                    Ok(runtime_args) => runtime_args,
                    Err(cve) => return TransferResult::Failure(TransferError::CLValue(cve)),
                }
            }
            TransferRequestArgs::Indirect(bita) => {
                let source_uref = match bita
                    .source()
                    .purse_uref(&mut tc.borrow_mut(), protocol_version)
                {
                    Ok(source_uref) => source_uref,
                    Err(tce) => return TransferResult::Failure(TransferError::TrackingCopy(tce)),
                };
                let target_uref = match bita
                    .target()
                    .purse_uref(&mut tc.borrow_mut(), protocol_version)
                {
                    Ok(target_uref) => target_uref,
                    Err(tce) => return TransferResult::Failure(TransferError::TrackingCopy(tce)),
                };
                let transfer_args = TransferArgs::new(
                    bita.to(),
                    source_uref,
                    target_uref,
                    bita.amount(),
                    bita.arg_id(),
                );
                match RuntimeArgs::try_from(transfer_args) {
                    Ok(runtime_args) => runtime_args,
                    Err(cve) => return TransferResult::Failure(TransferError::CLValue(cve)),
                }
            }
        };

        let remaining_spending_limit = match runtime_args.try_get_number(ARG_AMOUNT) {
            Ok(amount) => amount,
            Err(cve) => {
                debug!("failed to derive remaining_spending_limit");
                return TransferResult::Failure(TransferError::CLValue(cve));
            }
        };

        let mut runtime_args_builder = TransferRuntimeArgsBuilder::new(runtime_args);

        let transfer_target_mode = match runtime_args_builder
            .resolve_transfer_target_mode(protocol_version, Rc::clone(&tc))
        {
            Ok(transfer_target_mode) => transfer_target_mode,
            Err(error) => return TransferResult::Failure(error),
        };

        // On some private networks, transfers are restricted.
        // This means that they must either the source or target are an admin account.
        // This behavior is not used on public networks.
        if transfer_config.enforce_transfer_restrictions(&source_account_hash) {
            // if the source is an admin, enforce_transfer_restrictions == false
            // if the source is not an admin, enforce_transfer_restrictions == true,
            // and we must check to see if the target is an admin.
            // if the target is also not an admin, this transfer is not permitted.
            match transfer_target_mode.target_account_hash() {
                Some(target_account_hash) => {
                    let is_target_system_account =
                        target_account_hash == PublicKey::System.to_account_hash();
                    let is_target_administrator =
                        transfer_config.is_administrator(&target_account_hash);
                    if !(is_target_system_account || is_target_administrator) {
                        // Transferring from normal account to a purse doesn't work.
                        return TransferResult::Failure(TransferError::RestrictedTransferAttempted);
                    }
                }
                None => {
                    // can't allow this transfer because we are not sure if the target is an admin.
                    return TransferResult::Failure(TransferError::UnableToVerifyTargetIsAdmin);
                }
            }
        }

        let (entity_addr, runtime_footprint, entity_access_rights) = match tc
            .borrow_mut()
            .authorized_runtime_footprint_with_access_rights(
                protocol_version,
                source_account_hash,
                authorization_keys,
                &administrative_accounts,
            ) {
            Ok(ret) => ret,
            Err(tce) => {
                return TransferResult::Failure(TransferError::TrackingCopy(tce));
            }
        };
        let entity_key = if config.addressable_entity_enabled() {
            Key::AddressableEntity(entity_addr)
        } else {
            match entity_addr {
                EntityAddr::System(hash)
                | EntityAddr::SmartContract(hash)
                | EntityAddr::Package(hash) => Key::Hash(hash),
                EntityAddr::Account(hash) => Key::Account(AccountHash::new(hash)),
            }
        };
        let id = Id::Transaction(request.transaction_hash());
        let phase = Phase::Session;
        let address_generator = AddressGenerator::new(&id.seed(), phase);
        // IMPORTANT: this runtime _must_ use the payer's context.
        let mut runtime = RuntimeNative::new(
            config.clone(),
            id,
            Arc::new(RwLock::new(address_generator)),
            Rc::clone(&tc),
            source_account_hash,
            entity_key,
            runtime_footprint.clone(),
            entity_access_rights,
            remaining_spending_limit,
            phase,
        );

        match transfer_target_mode {
            TransferTargetMode::ExistingAccount { .. } | TransferTargetMode::PurseExists { .. } => {
                // Noop
            }
            TransferTargetMode::CreateAccount(account_hash) => {
                let main_purse = match runtime.mint(U512::zero()) {
                    Ok(uref) => uref,
                    Err(mint_error) => {
                        return TransferResult::Failure(TransferError::Mint(mint_error));
                    }
                };

                let account = Account::create(account_hash, NamedKeys::new(), main_purse);
                if let Err(tce) = tc
                    .borrow_mut()
                    .create_addressable_entity_from_account(account, protocol_version)
                {
                    return TransferResult::Failure(tce.into());
                }
            }
        }
        let transfer_args = match runtime_args_builder.build(
            &runtime_footprint,
            protocol_version,
            Rc::clone(&tc),
        ) {
            Ok(transfer_args) => transfer_args,
            Err(error) => return TransferResult::Failure(error),
        };
        if let Err(mint_error) = runtime.transfer(
            transfer_args.to(),
            transfer_args.source(),
            transfer_args.target(),
            transfer_args.amount(),
            transfer_args.arg_id(),
        ) {
            return TransferResult::Failure(TransferError::Mint(mint_error));
        }

        let transfers = runtime.into_transfers();

        let effects = tc.borrow_mut().effects();
        let cache = tc.borrow_mut().cache();

        TransferResult::Success {
            transfers,
            effects,
            cache,
        }
    }

    /// Direct burn.
    fn burn(&self, request: BurnRequest) -> BurnResult {
        let state_hash = request.state_hash();
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return BurnResult::RootNotFound,
            Err(err) => {
                return BurnResult::Failure(BurnError::TrackingCopy(TrackingCopyError::Storage(
                    err,
                )));
            }
        };

        let source_account_hash = request.initiator().account_hash();
        let protocol_version = request.protocol_version();
        if let Err(tce) = tc
            .borrow_mut()
            .migrate_account(source_account_hash, protocol_version)
        {
            return BurnResult::Failure(tce.into());
        }

        let authorization_keys = request.authorization_keys();

        let config = request.config();

        let runtime_args = match request.args() {
            BurnRequestArgs::Raw(runtime_args) => runtime_args.clone(),
            BurnRequestArgs::Explicit(transfer_args) => {
                match RuntimeArgs::try_from(*transfer_args) {
                    Ok(runtime_args) => runtime_args,
                    Err(cve) => return BurnResult::Failure(BurnError::CLValue(cve)),
                }
            }
        };

        let runtime_args_builder = BurnRuntimeArgsBuilder::new(runtime_args);

        let (entity_addr, mut footprint, mut entity_access_rights) = match tc
            .borrow_mut()
            .authorized_runtime_footprint_with_access_rights(
                protocol_version,
                source_account_hash,
                authorization_keys,
                &BTreeSet::default(),
            ) {
            Ok(ret) => ret,
            Err(tce) => {
                return BurnResult::Failure(BurnError::TrackingCopy(tce));
            }
        };
        let entity_key = if config.addressable_entity_enabled() {
            Key::AddressableEntity(entity_addr)
        } else {
            match entity_addr {
                EntityAddr::System(hash) | EntityAddr::SmartContract(hash) => Key::Hash(hash),
                EntityAddr::Account(hash) => Key::Account(AccountHash::new(hash)),
                EntityAddr::Package(hash_addr) => Key::Hash(hash_addr),
            }
        };

        // extend named keys with total supply
        match tc
            .borrow_mut()
            .system_contract_named_key(MINT, TOTAL_SUPPLY_KEY)
        {
            Ok(Some(k)) => {
                match k.as_uref() {
                    Some(uref) => entity_access_rights.extend(&[*uref]),
                    None => {
                        return BurnResult::Failure(BurnError::TrackingCopy(
                            TrackingCopyError::UnexpectedKeyVariant(k),
                        ));
                    }
                }
                footprint.insert_into_named_keys(TOTAL_SUPPLY_KEY.into(), k);
            }
            Ok(None) => {
                return BurnResult::Failure(BurnError::TrackingCopy(
                    TrackingCopyError::NamedKeyNotFound(TOTAL_SUPPLY_KEY.into()),
                ));
            }
            Err(tce) => {
                return BurnResult::Failure(BurnError::TrackingCopy(tce));
            }
        };
        let id = Id::Transaction(request.transaction_hash());
        let phase = Phase::Session;
        let address_generator = AddressGenerator::new(&id.seed(), phase);
        let burn_args = match runtime_args_builder.build(&footprint, Rc::clone(&tc)) {
            Ok(burn_args) => burn_args,
            Err(error) => return BurnResult::Failure(error),
        };

        // IMPORTANT: this runtime _must_ use the payer's context.
        let mut runtime = RuntimeNative::new(
            config.clone(),
            id,
            Arc::new(RwLock::new(address_generator)),
            Rc::clone(&tc),
            source_account_hash,
            entity_key,
            footprint.clone(),
            entity_access_rights,
            burn_args.amount(),
            phase,
        );

        if let Err(mint_error) = runtime.burn(burn_args.source(), burn_args.amount()) {
            return BurnResult::Failure(BurnError::Mint(mint_error));
        }

        let effects = tc.borrow_mut().effects();
        let cache = tc.borrow_mut().cache();

        BurnResult::Success { effects, cache }
    }

    /// Gets all values under a given key tag.
    fn tagged_values(&self, request: TaggedValuesRequest) -> TaggedValuesResult {
        let state_hash = request.state_hash();
        let mut tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return TaggedValuesResult::RootNotFound,
            Err(gse) => return TaggedValuesResult::Failure(TrackingCopyError::Storage(gse)),
        };

        let key_tag = request.key_tag();
        let keys = match tc.get_keys(&key_tag) {
            Ok(keys) => keys,
            Err(tce) => return TaggedValuesResult::Failure(tce),
        };

        let mut values = vec![];
        for key in keys {
            match tc.get(&key) {
                Ok(Some(value)) => {
                    values.push(value);
                }
                Ok(None) => {}
                Err(error) => return TaggedValuesResult::Failure(error),
            }
        }

        TaggedValuesResult::Success {
            values,
            selection: request.selection(),
        }
    }

    /// Gets all values under a given key prefix.
    /// Currently, this ignores the cache and only provides values from the trie.
    fn prefixed_values(&self, request: PrefixedValuesRequest) -> PrefixedValuesResult {
        let mut tc = match self.tracking_copy(request.state_hash()) {
            Ok(Some(tc)) => tc,
            Ok(None) => return PrefixedValuesResult::RootNotFound,
            Err(err) => return PrefixedValuesResult::Failure(TrackingCopyError::Storage(err)),
        };
        match tc.get_keys_by_prefix(request.key_prefix()) {
            Ok(keys) => {
                let mut values = Vec::with_capacity(keys.len());
                for key in keys {
                    match tc.get(&key) {
                        Ok(Some(value)) => values.push(value),
                        Ok(None) => {}
                        Err(error) => return PrefixedValuesResult::Failure(error),
                    }
                }
                PrefixedValuesResult::Success {
                    values,
                    key_prefix: request.key_prefix().clone(),
                }
            }
            Err(error) => PrefixedValuesResult::Failure(error),
        }
    }

    /// Reads a `Trie` from the state if it is present
    fn trie(&self, request: TrieRequest) -> TrieResult;

    /// Persists a trie element.
    fn put_trie(&self, request: PutTrieRequest) -> PutTrieResult;

    /// Finds all the children of `trie_raw` which aren't present in the state.
    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, GlobalStateError>;

    /// Gets the value of enable entity flag.
    fn enable_entity(&self) -> bool;
}

fn get_round_seigniorage_rate_data<T: StateProvider>(
    state_provider: &T,
    scr: &SystemHashRegistry,
    state_hash: Digest,
    addressable_entity_enabled: bool,
) -> RoundSeigniorageRateResult {
    let query_request = match scr.get(MINT).copied() {
        Some(mint_hash) => {
            let key = if !addressable_entity_enabled {
                Key::Hash(mint_hash)
            } else {
                Key::AddressableEntity(EntityAddr::System(mint_hash))
            };
            QueryRequest::new(
                state_hash,
                key,
                vec![ROUND_SEIGNIORAGE_RATE_KEY.to_string()],
            )
        }
        None => {
            error!("unexpected query failure; mint not found");
            return RoundSeigniorageRateResult::MintNotFound;
        }
    };

    match state_provider.query(query_request) {
        QueryResult::RootNotFound => RoundSeigniorageRateResult::RootNotFound,
        QueryResult::ValueNotFound(msg) => RoundSeigniorageRateResult::ValueNotFound(msg),
        QueryResult::Failure(tce) => RoundSeigniorageRateResult::Failure(tce),
        QueryResult::Success { value, proofs: _ } => {
            let cl_value = match value.into_cl_value() {
                Some(cl_value) => cl_value,
                None => {
                    error!("unexpected query failure; total supply is not a CLValue");
                    return RoundSeigniorageRateResult::Failure(
                        TrackingCopyError::UnexpectedStoredValueVariant,
                    );
                }
            };

            match cl_value.into_t() {
                Ok(rate) => RoundSeigniorageRateResult::Success { rate },
                Err(cve) => RoundSeigniorageRateResult::Failure(TrackingCopyError::CLValue(cve)),
            }
        }
    }
}

fn get_total_supply_data<T: StateProvider>(
    state_provider: &T,
    scr: &SystemHashRegistry,
    state_hash: Digest,
    addressable_entity_enabled: bool,
) -> TotalSupplyResult {
    let query_request = match scr.get(MINT).copied() {
        Some(mint_hash) => {
            let key = if !addressable_entity_enabled {
                Key::Hash(mint_hash)
            } else {
                Key::AddressableEntity(EntityAddr::System(mint_hash))
            };
            QueryRequest::new(state_hash, key, vec![TOTAL_SUPPLY_KEY.to_string()])
        }
        None => {
            error!("unexpected query failure; mint not found");
            return TotalSupplyResult::MintNotFound;
        }
    };
    match state_provider.query(query_request) {
        QueryResult::RootNotFound => TotalSupplyResult::RootNotFound,
        QueryResult::ValueNotFound(msg) => TotalSupplyResult::ValueNotFound(msg),
        QueryResult::Failure(tce) => TotalSupplyResult::Failure(tce),
        QueryResult::Success { value, proofs: _ } => {
            let cl_value = match value.into_cl_value() {
                Some(cl_value) => cl_value,
                None => {
                    error!("unexpected query failure; total supply is not a CLValue");
                    return TotalSupplyResult::Failure(
                        TrackingCopyError::UnexpectedStoredValueVariant,
                    );
                }
            };

            match cl_value.into_t() {
                Ok(total_supply) => TotalSupplyResult::Success { total_supply },
                Err(cve) => TotalSupplyResult::Failure(TrackingCopyError::CLValue(cve)),
            }
        }
    }
}

fn get_snapshot_data<T: StateProvider>(
    state_provider: &T,
    scr: &SystemHashRegistry,
    state_hash: Digest,
    addressable_entity_enabled: bool,
) -> SeigniorageRecipientsResult {
    let (snapshot_query_request, snapshot_version_query_request) =
        match build_query_requests(scr, state_hash, addressable_entity_enabled) {
            Ok(res) => res,
            Err(res) => return res,
        };

    // check if snapshot version flag is present
    let snapshot_version: Option<u8> =
        match query_snapshot_version(state_provider, snapshot_version_query_request) {
            Ok(value) => value,
            Err(value) => return value,
        };

    let snapshot = match query_snapshot(state_provider, snapshot_version, snapshot_query_request) {
        Ok(snapshot) => snapshot,
        Err(value) => return value,
    };

    SeigniorageRecipientsResult::Success {
        seigniorage_recipients: snapshot,
    }
}

fn query_snapshot<T: StateProvider>(
    state_provider: &T,
    snapshot_version: Option<u8>,
    snapshot_query_request: QueryRequest,
) -> Result<SeigniorageRecipientsSnapshot, SeigniorageRecipientsResult> {
    match state_provider.query(snapshot_query_request) {
        QueryResult::RootNotFound => Err(SeigniorageRecipientsResult::RootNotFound),
        QueryResult::Failure(error) => {
            error!(?error, "unexpected tracking copy error");
            Err(SeigniorageRecipientsResult::Failure(error))
        }
        QueryResult::ValueNotFound(msg) => {
            error!(%msg, "value not found");
            Err(SeigniorageRecipientsResult::ValueNotFound(msg))
        }
        QueryResult::Success { value, proofs: _ } => {
            let cl_value = match value.into_cl_value() {
                Some(snapshot_cl_value) => snapshot_cl_value,
                None => {
                    error!("unexpected query failure; seigniorage recipients snapshot is not a CLValue");
                    return Err(SeigniorageRecipientsResult::Failure(
                        TrackingCopyError::UnexpectedStoredValueVariant,
                    ));
                }
            };

            match snapshot_version {
                Some(_) => {
                    let snapshot = match cl_value.into_t() {
                        Ok(snapshot) => snapshot,
                        Err(cve) => {
                            error!("Failed to convert snapshot from CLValue");
                            return Err(SeigniorageRecipientsResult::Failure(
                                TrackingCopyError::CLValue(cve),
                            ));
                        }
                    };
                    Ok(SeigniorageRecipientsSnapshot::V2(snapshot))
                }
                None => {
                    let snapshot = match cl_value.into_t() {
                        Ok(snapshot) => snapshot,
                        Err(cve) => {
                            error!("Failed to convert snapshot from CLValue");
                            return Err(SeigniorageRecipientsResult::Failure(
                                TrackingCopyError::CLValue(cve),
                            ));
                        }
                    };
                    Ok(SeigniorageRecipientsSnapshot::V1(snapshot))
                }
            }
        }
    }
}

fn query_snapshot_version<T: StateProvider>(
    state_provider: &T,
    snapshot_version_query_request: QueryRequest,
) -> Result<Option<u8>, SeigniorageRecipientsResult> {
    match state_provider.query(snapshot_version_query_request) {
        QueryResult::RootNotFound => Err(SeigniorageRecipientsResult::RootNotFound),
        QueryResult::Failure(error) => {
            error!(?error, "unexpected tracking copy error");
            Err(SeigniorageRecipientsResult::Failure(error))
        }
        QueryResult::ValueNotFound(_msg) => Ok(None),
        QueryResult::Success { value, proofs: _ } => {
            let cl_value = match value.into_cl_value() {
                Some(snapshot_version_cl_value) => snapshot_version_cl_value,
                None => {
                    error!("unexpected query failure; seigniorage recipients snapshot version is not a CLValue");
                    return Err(SeigniorageRecipientsResult::Failure(
                        TrackingCopyError::UnexpectedStoredValueVariant,
                    ));
                }
            };
            match cl_value.into_t() {
                Ok(snapshot_version) => Ok(Some(snapshot_version)),
                Err(cve) => Err(SeigniorageRecipientsResult::Failure(
                    TrackingCopyError::CLValue(cve),
                )),
            }
        }
    }
}

fn build_query_requests(
    scr: &SystemHashRegistry,
    state_hash: Digest,
    addressable_entity_enabled: bool,
) -> Result<(QueryRequest, QueryRequest), SeigniorageRecipientsResult> {
    match scr.get(AUCTION).copied() {
        Some(auction_hash) => {
            let key = if !addressable_entity_enabled {
                Key::Hash(auction_hash)
            } else {
                Key::AddressableEntity(EntityAddr::System(auction_hash))
            };
            Ok((
                QueryRequest::new(
                    state_hash,
                    key,
                    vec![SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY.to_string()],
                ),
                QueryRequest::new(
                    state_hash,
                    key,
                    vec![SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY.to_string()],
                ),
            ))
        }
        None => Err(SeigniorageRecipientsResult::AuctionNotFound),
    }
}

/// Write multiple key/stored value pairs to the store in a single rw transaction.
pub fn put_stored_values<'a, R, S, E>(
    environment: &'a R,
    store: &S,
    prestate_hash: Digest,
    stored_values: Vec<(Key, StoredValue)>,
) -> Result<Digest, E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<Key, StoredValue>,
    S::Error: From<R::Error>,
    E: From<R::Error>
        + From<S::Error>
        + From<bytesrepr::Error>
        + From<CommitError>
        + From<TrieStoreCacheError>,
{
    let mut txn = environment.create_read_write_txn()?;
    let state_root = prestate_hash;
    let maybe_root: Option<Trie<Key, StoredValue>> = store.get(&txn, &state_root)?;
    if maybe_root.is_none() {
        return Err(CommitError::RootNotFound(prestate_hash).into());
    };

    let state_root =
        batch_write::<_, _, _, _, _, E>(&mut txn, store, &state_root, stored_values.into_iter())?;
    txn.commit()?;
    Ok(state_root)
}

/// Commit `effects` to the store.
pub fn commit<'a, R, S, E>(
    environment: &'a R,
    store: &S,
    prestate_hash: Digest,
    effects: Effects,
) -> Result<Digest, E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<Key, StoredValue>,
    S::Error: From<R::Error>,
    E: From<R::Error>
        + From<S::Error>
        + From<bytesrepr::Error>
        + From<CommitError>
        + From<GlobalStateError>, /* even tho E is currently always GSE, this is required to
                                   * satisfy the compiler */
{
    let mut txn = environment.create_read_write_txn()?;
    let mut state_root = prestate_hash;

    let maybe_root: Option<Trie<Key, StoredValue>> = store.get(&txn, &state_root)?;

    if maybe_root.is_none() {
        return Err(CommitError::RootNotFound(prestate_hash).into());
    };

    for (key, kind) in effects.value().into_iter().map(TransformV2::destructure) {
        let read_result = read::<_, _, _, _, E>(&txn, store, &state_root, &key)?;

        let instruction = match (read_result, kind) {
            (_, TransformKindV2::Identity) => {
                // effectively a noop.
                continue;
            }
            (_, TransformKindV2::Ret(_)) => {
                // Ret transforms are not committed to global state.
                continue;
            }
            (ReadResult::NotFound, TransformKindV2::Write(new_value)) => {
                TransformInstruction::store(new_value)
            }
            (ReadResult::NotFound, TransformKindV2::Prune(key)) => {
                // effectively a noop.
                debug!(
                    ?state_root,
                    ?key,
                    "commit: attempt to prune nonexistent record; this may happen if a key is both added and pruned in the same commit."
                );
                continue;
            }
            (ReadResult::NotFound, transform_kind) => {
                error!(
                    ?state_root,
                    ?key,
                    ?transform_kind,
                    "commit: key not found while attempting to apply transform"
                );
                return Err(CommitError::KeyNotFound(key).into());
            }
            (ReadResult::Found(current_value), transform_kind) => {
                match transform_kind.apply(current_value) {
                    Ok(instruction) => instruction,
                    Err(err) => {
                        error!(
                            ?state_root,
                            ?key,
                            ?err,
                            "commit: key found, but could not apply transform"
                        );
                        return Err(CommitError::TransformError(err).into());
                    }
                }
            }
            (ReadResult::RootNotFound, transform_kind) => {
                error!(
                    ?state_root,
                    ?key,
                    ?transform_kind,
                    "commit: failed to read state root while processing transform"
                );
                return Err(CommitError::ReadRootNotFound(state_root).into());
            }
        };

        match instruction {
            TransformInstruction::Store(value) => {
                let write_result =
                    write::<_, _, _, _, E>(&mut txn, store, &state_root, &key, &value)?;

                match write_result {
                    WriteResult::Written(root_hash) => {
                        state_root = root_hash;
                    }
                    WriteResult::AlreadyExists => (),
                    WriteResult::RootNotFound => {
                        error!(?state_root, ?key, ?value, "commit: root not found");
                        return Err(CommitError::WriteRootNotFound(state_root).into());
                    }
                }
            }
            TransformInstruction::Prune(key) => {
                let prune_result = prune::<_, _, _, _, E>(&mut txn, store, &state_root, &key)?;

                match prune_result {
                    TriePruneResult::Pruned(root_hash) => {
                        state_root = root_hash;
                    }
                    TriePruneResult::MissingKey => {
                        warn!("commit: pruning attempt failed for {}", key);
                    }
                    TriePruneResult::RootNotFound => {
                        error!(?state_root, ?key, "commit: root not found");
                        return Err(CommitError::WriteRootNotFound(state_root).into());
                    }
                    TriePruneResult::Failure(gse) => {
                        return Err(gse.into()); // currently this is always reflexive
                    }
                }
            }
        }
    }

    txn.commit()?;

    Ok(state_root)
}

fn find_historic_delegator_bids<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tc: &mut TrackingCopy<R>,
    validator_public_key: &PublicKey,
    delegator_public_key: &PublicKey,
) -> Result<Vec<BidKind>, TrackingCopyError> {
    let validator_account_hash = validator_public_key.to_account_hash();
    let validator_account_hash_bytes = match validator_account_hash.to_bytes() {
        Ok(account_hash_bytes) => account_hash_bytes,
        Err(e) => return Err(TrackingCopyError::BytesRepr(e)),
    };
    let bid_key_bytes = [
        vec![KeyTag::Bid as u8],
        validator_account_hash_bytes.clone(),
    ]
    .concat();
    let keys = tc.get_by_byte_prefix(&bid_key_bytes)?;
    let mut bids = vec![];
    for key in keys {
        match tc.get(&key)? {
            Some(StoredValue::Bid(bid)) => {
                if let Some(delegator) = bid.delegators().get(delegator_public_key) {
                    let delegator_kind = match bid.vesting_schedule() {
                        Some(vesting_schedule) => {
                            let mut delegator_bid = DelegatorBid::locked(
                                DelegatorKind::PublicKey(delegator_public_key.clone()),
                                delegator.staked_amount(),
                                *delegator.bonding_purse(),
                                validator_public_key.clone(),
                                vesting_schedule.initial_release_timestamp_millis(),
                            );
                            if let Some(output_vesting_schedule) =
                                delegator_bid.vesting_schedule_mut()
                            {
                                *output_vesting_schedule = vesting_schedule.clone();
                            }
                            delegator_bid
                        }
                        None => DelegatorBid::unlocked(
                            DelegatorKind::PublicKey(delegator_public_key.clone()),
                            delegator.staked_amount(),
                            *delegator.bonding_purse(),
                            validator_public_key.clone(),
                        ),
                    };

                    let bid_kind = BidKind::Delegator(Box::new(delegator_kind));
                    bids.push(bid_kind);
                }
            }
            Some(_) => {
                return Err(TrackingCopyError::UnexpectedStoredValueVariant);
            }
            None => {
                return Err(TrackingCopyError::ValueNotFound(format!(
                    "BidKind entry with key {} not found",
                    key
                )));
            }
        }
    }
    Ok(bids)
}

fn find_contemporary_delegator_bids<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tc: &mut TrackingCopy<R>,
    validator_account_hash: &AccountHash,
    delegator_kind: &DelegatorKind,
) -> Result<Vec<BidKind>, TrackingCopyError> {
    let validator_account_hash_bytes = match validator_account_hash.to_bytes() {
        Ok(account_hash_bytes) => account_hash_bytes,
        Err(e) => return Err(TrackingCopyError::BytesRepr(e)),
    };
    let mut bids = vec![];
    let mut keys = BTreeSet::new();
    match delegator_kind {
        DelegatorKind::PublicKey(public_key) => {
            let delegator_account_hash = public_key.to_account_hash();
            let delegator_account_hash_bytes = match delegator_account_hash.to_bytes() {
                Ok(delegator_account_hash_bytes) => delegator_account_hash_bytes,
                Err(e) => return Err(TrackingCopyError::BytesRepr(e)),
            };

            for infix in BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_ACCOUNT {
                let key_bytes = [
                    vec![KeyTag::BidAddr as u8, (*infix) as u8],
                    validator_account_hash_bytes.clone(),
                    delegator_account_hash_bytes.clone(),
                ]
                .concat();
                match tc.get_by_byte_prefix(&key_bytes) {
                    Ok(mut k) => {
                        keys.append(&mut k);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        DelegatorKind::Purse(purse) => {
            for infix in BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_PURSE {
                let validator_bytes = validator_account_hash_bytes.clone();
                let key_bytes = [
                    vec![KeyTag::BidAddr as u8, (*infix) as u8],
                    validator_bytes,
                    purse.to_vec(),
                ]
                .concat();
                match tc.get_by_byte_prefix(&key_bytes) {
                    Ok(mut k) => {
                        keys.append(&mut k);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }

    for key in keys {
        match tc.get(&key)? {
            Some(StoredValue::BidKind(bid_kind)) => {
                bids.push(bid_kind);
            }
            Some(_) => {
                return Err(TrackingCopyError::UnexpectedStoredValueVariant);
            }
            None => {
                return Err(TrackingCopyError::ValueNotFound(format!(
                    "BidKind entry with key {} not found",
                    key
                )));
            }
        }
    }
    Ok(bids)
}

fn find_contemporary_validator_bids<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tc: &mut TrackingCopy<R>,
    account_hash: &AccountHash,
) -> ValidatorBidsResult {
    let account_hash_bytes = match account_hash.to_bytes() {
        Ok(account_hash_bytes) => account_hash_bytes,
        Err(e) => return ValidatorBidsResult::Failure(TrackingCopyError::BytesRepr(e)),
    };
    let mut bids = vec![];
    let mut keys = BTreeSet::new();
    for infix in BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_VALIDATORS {
        let key_bytes = [
            vec![KeyTag::BidAddr as u8, (*infix) as u8],
            account_hash_bytes.clone(),
        ]
        .concat();
        match tc.get_by_byte_prefix(&key_bytes) {
            Ok(mut k) => {
                keys.append(&mut k);
            }
            Err(e) => return ValidatorBidsResult::Failure(e),
        }
    }
    for key in keys {
        match tc.get(&key) {
            Ok(Some(StoredValue::BidKind(bid_kind))) => {
                bids.push(bid_kind);
            }
            Ok(Some(_)) => {
                return ValidatorBidsResult::Failure(
                    TrackingCopyError::UnexpectedStoredValueVariant,
                );
            }
            Ok(None) => {
                return ValidatorBidsResult::Failure(TrackingCopyError::ValueNotFound(format!(
                    "BidKind entry with key {} not found",
                    key
                )))
            }
            Err(error) => return ValidatorBidsResult::Failure(error),
        }
    }
    ValidatorBidsResult::Success { bids }
}

fn find_historic_validator_bids<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tc: &mut TrackingCopy<R>,
    account_hash: &AccountHash,
) -> ValidatorBidsResult {
    let account_hash_bytes = match account_hash.to_bytes() {
        Ok(account_hash_bytes) => account_hash_bytes,
        Err(e) => return ValidatorBidsResult::Failure(TrackingCopyError::BytesRepr(e)),
    };
    let mut bids = vec![];
    let key_bytes = [vec![KeyTag::Bid as u8], account_hash_bytes.clone()].concat();
    let keys = match tc.get_by_byte_prefix(&key_bytes) {
        Ok(keys) => keys,
        Err(e) => return ValidatorBidsResult::Failure(e),
    };

    for key in keys {
        //Technically there should never be more than one key here
        match tc.get(&key) {
            Ok(Some(StoredValue::Bid(bid))) => {
                let validator_bid = BidKind::Unified(bid);
                bids.push(validator_bid);
                let key_bytes = [vec![KeyTag::Unbond as u8], account_hash_bytes.clone()].concat();
                let unbond_keys = match tc.get_by_byte_prefix(&key_bytes) {
                    Ok(keys) => keys,
                    Err(e) => return ValidatorBidsResult::Failure(e),
                };
                for unbond_key in unbond_keys {
                    match tc.get(&unbond_key) {
                        Ok(Some(StoredValue::Unbonding(unbonding_purses))) => {
                            match try_rewrap_unbonding_purses(unbonding_purses) {
                                Ok(mut unbonding_bid_kinds) => bids.append(&mut unbonding_bid_kinds),
                                Err(UnbondingRewrapError::AmbiguousValidatorKey) => {
                                    return ValidatorBidsResult::Failure(
                                        TrackingCopyError::ErrorWhenRewraping("Could not map historical Unbonding records due to ambiguous validator keys".to_owned()),
                                    )
                                }
                            }
                        }
                        Ok(Some(_)) => {
                            return ValidatorBidsResult::Failure(
                                TrackingCopyError::UnexpectedStoredValueVariant,
                            );
                        }
                        Ok(None) => {
                            return ValidatorBidsResult::Failure(TrackingCopyError::ValueNotFound(
                                format!("Unbonding entry with key {} not found", unbond_key),
                            ))
                        }
                        Err(err) => return ValidatorBidsResult::Failure(err),
                    }
                }
            }
            Ok(Some(_)) => {
                return ValidatorBidsResult::Failure(
                    TrackingCopyError::UnexpectedStoredValueVariant,
                );
            }
            Ok(None) => {
                return ValidatorBidsResult::Failure(TrackingCopyError::ValueNotFound(format!(
                    "Bid entry with key {} not found",
                    key
                )))
            }
            Err(error) => return ValidatorBidsResult::Failure(error),
        }
    }

    ValidatorBidsResult::Success { bids }
}

enum UnbondingRewrapError {
    AmbiguousValidatorKey,
}
fn try_rewrap_unbonding_purses(
    unbonding_purses: Vec<UnbondingPurse>,
) -> Result<Vec<BidKind>, UnbondingRewrapError> {
    let base_validator_public_key = match unbonding_purses.first() {
        None => return Ok(vec![]),
        Some(purse) => purse.validator_public_key().clone(),
    };
    let mut unbonding_purses_map: BTreeMap<UnbondKind, Vec<UnbondingPurse>> = BTreeMap::new();

    for unbonding_purse in unbonding_purses {
        if !unbonding_purse
            .validator_public_key()
            .eq(&base_validator_public_key)
        {
            return Err(UnbondingRewrapError::AmbiguousValidatorKey);
        }
        let unbond_kind =
            if unbonding_purse.validator_public_key() == unbonding_purse.unbonder_public_key() {
                UnbondKind::Validator(unbonding_purse.validator_public_key().clone())
            } else {
                UnbondKind::DelegatedPublicKey(unbonding_purse.unbonder_public_key().clone())
            };

        match unbonding_purses_map.entry(unbond_kind) {
            std::collections::btree_map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(vec![unbonding_purse]);
            }
            std::collections::btree_map::Entry::Occupied(occupied_entry) => {
                occupied_entry.into_mut().push(unbonding_purse);
            }
        }
    }
    let mut bid_kinds = vec![];
    for (unbonding_kind, purses) in unbonding_purses_map {
        if let Some(purse) = purses.first() {
            let validator_key = purse.validator_public_key().clone();
            let mut eras = vec![];
            for unbonding_purse in purses {
                eras.push(UnbondEra::new(
                    *unbonding_purse.bonding_purse(),
                    unbonding_purse.era_of_creation(),
                    *unbonding_purse.amount(),
                    unbonding_purse.new_validator().clone(),
                ));
            }
            let unbond = Unbond::new(validator_key, unbonding_kind, eras);
            bid_kinds.push(BidKind::Unbond(Box::new(unbond)));
        }
    }
    Ok(bid_kinds)
}

#[cfg(test)]
mod tests {
    use crate::global_state::state::{
        BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_ACCOUNT,
        BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_PURSE,
        BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_VALIDATORS,
    };
    use casper_types::system::auction::BidAddrTag;
    use strum::IntoEnumIterator;

    #[test]
    fn validate_bid_addr_tags_relevant_for_contemporary_validators() {
        // The BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_VALIDATORS constant should contain
        // all the tags that can be found by using a validator account hash prefix.
        // We should think about this as BidAddrTag entries that are "related" to a
        // BidKind::Validator entity. If a new BidAddrTag variant is added
        // this constant should be considered and expanded if necessary.
        let number_of_bid_addr_tags = BidAddrTag::iter().len();
        assert_eq!(BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_VALIDATORS.len(), 8);
        assert_eq!(number_of_bid_addr_tags, 10);
    }

    #[test]
    fn validate_bid_addr_tags_relevant_for_contemporary_delegators_account() {
        // The BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_ACCOUNT constant should contain
        // all the tags that can be found by using validator+delegator_public_key prefix.
        // We should think about this as BidAddrTag entries that are "related" to a
        // BidKind::Delegator entity that is of DelegatorKind::PublicKey. If a new BidAddrTag
        // variant is added this constant should be considered and expanded if necessary.
        let number_of_bid_addr_tags = BidAddrTag::iter().len();
        assert_eq!(
            BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_ACCOUNT.len(),
            3
        );
        assert_eq!(number_of_bid_addr_tags, 10);
    }

    #[test]
    fn validate_bid_addr_tags_relevant_for_contemporary_delegators_purse() {
        // The BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_PURSE constant should contain
        // all the tags that can be found by using validator+purse_uref prefix.
        // We should think about this as BidAddrTag entries that are "related" to a
        // BidKind::Delegator entity that is of DelegatorKind::Purse. If a new BidAddrTag
        // variant is added this constant should be considered and expanded if necessary.
        let number_of_bid_addr_tags = BidAddrTag::iter().len();
        assert_eq!(
            BID_ADDR_TAGS_RELEVANT_FOR_CONTEMPORARY_DELEGATORS_PURSE.len(),
            3
        );
        assert_eq!(number_of_bid_addr_tags, 10);
    }
}
