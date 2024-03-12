//! Global state.

/// Lmdb implementation of global state.
pub mod lmdb;

/// Lmdb implementation of global state with cache.
pub mod scratch;

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    convert::TryFrom,
    rc::Rc,
};
use tracing::{debug, error, warn};

#[cfg(test)]
pub use self::lmdb::make_temporary_global_state;

use casper_types::{
    addressable_entity::{EntityKindTag, NamedKeys},
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    execution::{Effects, TransformError, TransformInstruction, TransformKindV2, TransformV2},
    global_state::TrieMerkleProof,
    system,
    system::{
        auction::SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
        handle_payment::ACCUMULATION_PURSE_KEY,
        mint::{ARG_AMOUNT, ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY},
        AUCTION, HANDLE_PAYMENT, MINT,
    },
    AccessRights, Account, AddressableEntity, DeployHash, Digest, EntityAddr, Key, KeyTag, Phase,
    PublicKey, RuntimeArgs, StoredValue, TransactionHash, TransactionV1Hash, U512,
};

use crate::{
    data_access_layer::{
        bidding::{AuctionMethodRet, BiddingRequest, BiddingResult},
        era_validators::EraValidatorsResult,
        tagged_values::{TaggedValuesRequest, TaggedValuesResult},
        transfer::{TransferRequest, TransferRequestArgs, TransferResult},
        AddressableEntityRequest, AddressableEntityResult, AuctionMethod, BalanceIdentifier,
        BalanceRequest, BalanceResult, BidsRequest, BidsResult, BlockRewardsError,
        BlockRewardsRequest, BlockRewardsResult, EraValidatorsRequest,
        ExecutionResultsChecksumRequest, ExecutionResultsChecksumResult, FeeError, FeeRequest,
        FeeResult, FlushRequest, FlushResult, GenesisRequest, GenesisResult,
        ProtocolUpgradeRequest, ProtocolUpgradeResult, PruneRequest, PruneResult, PutTrieRequest,
        PutTrieResult, QueryRequest, QueryResult, RoundSeigniorageRateRequest,
        RoundSeigniorageRateResult, StepError, StepRequest, StepResult,
        SystemEntityRegistryPayload, SystemEntityRegistryRequest, SystemEntityRegistryResult,
        SystemEntityRegistrySelector, TotalSupplyRequest, TotalSupplyResult, TrieRequest,
        TrieResult, EXECUTION_RESULTS_CHECKSUM_NAME,
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
        auction,
        auction::Auction,
        genesis::{GenesisError, GenesisInstaller},
        mint::Mint,
        protocol_upgrade::{ProtocolUpgradeError, ProtocolUpgrader},
        runtime_native::RuntimeNative,
        transfer::{
            NewTransferTargetMode, TransferArgs, TransferError, TransferRuntimeArgsBuilder,
        },
    },
    tracking_copy::{TrackingCopy, TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
};

/// A trait expressing the reading of state. This trait is used to abstract the underlying store.
pub trait StateReader<K, V> {
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

pub trait ScratchProvider: CommitProvider {
    fn get_scratch_global_state(&self) -> ScratchGlobalState;
    fn write_scratch_to_db(
        &self,
        state_root_hash: Digest,
        scratch_global_state: ScratchGlobalState,
    ) -> Result<Digest, GlobalStateError>;
    fn prune_keys(&self, state_root_hash: Digest, keys: &[Key]) -> TriePruneResult;
}

/// Provides `commit` method.
pub trait CommitProvider: StateProvider {
    /// Applies changes and returns a new post state hash.
    /// block_hash is used for computing a deterministic and unique keys.
    fn commit(&self, state_hash: Digest, effects: Effects) -> Result<Digest, GlobalStateError>;

    /// Runs and commits the genesis process, once per network.
    fn genesis(&self, request: GenesisRequest) -> GenesisResult {
        let initial_root = self.empty_root();
        let tc = match self.tracking_copy(initial_root) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return GenesisResult::Fatal("state uninitialized".to_string()),
            Err(err) => {
                return GenesisResult::Failure(GenesisError::TrackingCopy(
                    TrackingCopyError::Storage(err),
                ))
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
        match self.commit(initial_root, effects.clone()) {
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
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return ProtocolUpgradeResult::RootNotFound,
            Err(err) => {
                return ProtocolUpgradeResult::Failure(ProtocolUpgradeError::TrackingCopy(
                    TrackingCopyError::Storage(err),
                ))
            }
        };

        let protocol_upgrader: ProtocolUpgrader<Self> =
            ProtocolUpgrader::new(request.config().clone(), tc.clone());

        if let Err(err) = protocol_upgrader.upgrade(pre_state_hash) {
            return err.into();
        }

        let effects = tc.borrow().effects();

        // commit
        match self.commit(pre_state_hash, effects.clone()) {
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

        match self.commit(pre_state_hash, effects.clone()) {
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
                )))
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
                    ))
                }
            };
            match &mut protocol_version.into_bytes() {
                Ok(next) => bytes.append(next),
                Err(bre) => {
                    return StepResult::Failure(StepError::TrackingCopy(
                        TrackingCopyError::BytesRepr(*bre),
                    ))
                }
            };
            match &mut request.next_era_id().into_bytes() {
                Ok(next) => bytes.append(next),
                Err(bre) => {
                    return StepResult::Failure(StepError::TrackingCopy(
                        TrackingCopyError::BytesRepr(*bre),
                    ))
                }
            };

            crate::system::runtime_native::Id::Seed(bytes)
        };

        let config = request.config();
        // this runtime uses the system's context
        let mut runtime = match RuntimeNative::new_system_runtime(
            config.clone(),
            protocol_version,
            seed,
            Rc::clone(&tc),
            Phase::Session,
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
        let minimum_delegation_amount = config.minimum_delegation_amount();

        if let Err(err) = runtime.run_auction(
            era_end_timestamp_millis,
            evicted_validators,
            max_delegators_per_validator,
            minimum_delegation_amount,
        ) {
            error!("{}", err);
            return StepResult::Failure(StepError::Auction);
        }

        let effects = tc.borrow().effects();

        match self.commit(state_hash, effects.clone()) {
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
            warn!("rewards are empty");
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
                ))
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
                    ))
                }
            };
            match &mut protocol_version.into_bytes() {
                Ok(next) => bytes.append(next),
                Err(bre) => {
                    return BlockRewardsResult::Failure(BlockRewardsError::TrackingCopy(
                        TrackingCopyError::BytesRepr(*bre),
                    ))
                }
            };

            crate::system::runtime_native::Id::Seed(bytes)
        };

        // this runtime uses the system's context
        let mut runtime = match RuntimeNative::new_system_runtime(
            config.clone(),
            protocol_version,
            seed,
            Rc::clone(&tc),
            Phase::Session,
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
        }

        let effects = tc.borrow().effects();

        match self.commit(state_hash, effects.clone()) {
            Ok(post_state_hash) => BlockRewardsResult::Success {
                post_state_hash,
                effects,
            },
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

        let administrative_accounts = match request.administrative_accounts() {
            Some(administrative_accounts) => administrative_accounts,
            None => {
                return FeeResult::Failure(FeeError::AdministrativeAccountsNotFound);
            }
        };

        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
            Ok(None) => return FeeResult::RootNotFound,
            Err(gse) => {
                return FeeResult::Failure(FeeError::TrackingCopy(TrackingCopyError::Storage(gse)))
            }
        };

        // need the accumulation purse
        let entity_hash = match tc.borrow_mut().get_system_entity_registry() {
            Ok(scr) => match scr.get(HANDLE_PAYMENT).copied() {
                Some(entity_hash) => entity_hash,
                None => {
                    return FeeResult::Failure(FeeError::RegistryEntryNotFound(
                        HANDLE_PAYMENT.to_string(),
                    ))
                }
            },
            Err(tce) => return FeeResult::Failure(FeeError::TrackingCopy(tce)),
        };
        let named_keys = match tc
            .borrow_mut()
            .get_named_keys(EntityAddr::System(entity_hash.value()))
        {
            Ok(named_keys) => named_keys,
            Err(tce) => return FeeResult::Failure(FeeError::TrackingCopy(tce)),
        };

        let accumulation_purse = match named_keys.get(ACCUMULATION_PURSE_KEY) {
            Some(key) => {
                if let Key::URef(uref) = key {
                    *uref
                } else {
                    return FeeResult::Failure(FeeError::TrackingCopy(
                        TrackingCopyError::UnexpectedKeyVariant(*key),
                    ));
                }
            }
            None => {
                return FeeResult::Failure(FeeError::TrackingCopy(
                    TrackingCopyError::NamedKeyNotFound(ACCUMULATION_PURSE_KEY.to_string()),
                ))
            }
        };

        let protocol_version = request.protocol_version();

        let accumulated_balance = {
            let balance_req =
                BalanceRequest::from_purse(state_hash, protocol_version, accumulation_purse);
            let balance_result = self.balance(balance_req);
            match balance_result {
                BalanceResult::RootNotFound => {
                    return FeeResult::RootNotFound;
                }
                BalanceResult::Failure(tce) => {
                    return FeeResult::Failure(FeeError::TrackingCopy(tce));
                }
                BalanceResult::Success { motes, .. } => motes,
            }
        };

        let mut current_state_hash = state_hash;
        let mut effects = Effects::new();
        let mut transfers = vec![];
        let recipient_count = U512::from(administrative_accounts.len());

        // distribute fees to administrators
        // this is basically just a series of transfers from the accumulated purse to
        // configured accounts. this is a behavior used by some (but not all) private chains.
        if let Some(fee_portion) = accumulated_balance.checked_div(recipient_count) {
            if fee_portion.is_zero() {
                // If there are no fees to be paid out, it is effectively noop
                return FeeResult::Success {
                    effects: Effects::default(),
                    post_state_hash: state_hash,
                    transfers: vec![],
                };
            }

            let system_account_key = PublicKey::System;
            let id = {
                let mut bytes = match request.block_time().into_bytes() {
                    Ok(bytes) => bytes,
                    Err(bre) => {
                        return FeeResult::Failure(FeeError::TrackingCopy(
                            TrackingCopyError::BytesRepr(bre),
                        ))
                    }
                };
                match &mut state_hash.into_bytes() {
                    Ok(more_bytes) => bytes.append(more_bytes),
                    Err(bre) => {
                        return FeeResult::Failure(FeeError::TrackingCopy(
                            TrackingCopyError::BytesRepr(*bre),
                        ))
                    }
                };

                crate::system::runtime_native::Id::Seed(bytes)
            };

            let config = request.config();
            let block_time = request.block_time();
            let authorization_keys = {
                let mut auth_keys = BTreeSet::new();
                auth_keys.insert(system_account_key.to_account_hash());
                auth_keys
            };

            // TODO: the transfer logic needs to be tweaked once Fraser's logic w/ version handling
            // merges
            let tmp_hash = match TransactionV1Hash::from_bytes(&id.seed()) {
                Ok((hash, _rem)) => TransactionHash::V1(hash),
                Err(bre) => {
                    return FeeResult::Failure(FeeError::TrackingCopy(
                        TrackingCopyError::BytesRepr(bre),
                    ))
                }
            };
            for target in administrative_accounts {
                let target_purse = match tc
                    .borrow_mut()
                    .get_addressable_entity_by_account_hash(protocol_version, *target)
                {
                    Ok(entity) => entity.main_purse(),
                    Err(tce) => return FeeResult::Failure(FeeError::TrackingCopy(tce)),
                };
                let args = TransferArgs::new(
                    Some(*target),
                    accumulation_purse,
                    target_purse,
                    fee_portion,
                    None,
                );

                let transfer_req = TransferRequest::new(
                    config.clone(),
                    current_state_hash,
                    block_time,
                    protocol_version,
                    tmp_hash,
                    system_account_key.to_account_hash(),
                    authorization_keys.clone(),
                    args,
                    U512::zero(),
                );
                match self.transfer(transfer_req) {
                    TransferResult::RootNotFound => return FeeResult::RootNotFound,
                    TransferResult::Failure(transfer_error) => {
                        return FeeResult::Failure(FeeError::Transfer(transfer_error))
                    }
                    TransferResult::Success {
                        effects: transfer_effects,
                        transfers: more_transfers,
                    } => match self.commit(current_state_hash, transfer_effects.clone()) {
                        Ok(post_state_hash) => {
                            current_state_hash = post_state_hash;
                            effects.append(transfer_effects);
                            transfers.extend(more_transfers);
                        }
                        Err(gse) => {
                            return FeeResult::Failure(FeeError::TrackingCopy(
                                TrackingCopyError::Storage(gse),
                            ))
                        }
                    },
                }
            }

            return FeeResult::Success {
                post_state_hash: current_state_hash,
                effects,
                transfers,
            };
        }
        FeeResult::Failure(FeeError::NoFeesDistributed)
    }

    /// Direct biddings.
    fn bidding(&self, request: BiddingRequest) -> BiddingResult {
        let state_hash = request.state_hash();
        let tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return BiddingResult::RootNotFound,
            Err(err) => return BiddingResult::Failure(TrackingCopyError::Storage(err)),
        };

        let protocol_version = request.protocol_version();
        let config = request.config();
        let id = crate::system::runtime_native::Id::Transaction(request.transaction_hash());

        let initiating_address = request.address();
        let authorization_keys = request.authorization_keys();
        let transfer_config = config.transfer_config();
        let administrative_accounts = transfer_config.administrative_accounts();
        let (entity, entity_named_keys, entity_access_rights) =
            match tc.borrow_mut().resolved_entity(
                protocol_version,
                initiating_address,
                authorization_keys,
                &administrative_accounts,
            ) {
                Ok(ret) => ret,
                Err(tce) => {
                    return BiddingResult::Failure(tce);
                }
            };

        // IMPORTANT: this runtime _must_ use the initiators's context.
        let mut runtime = RuntimeNative::new(
            protocol_version,
            config.clone(),
            id,
            Rc::clone(&tc),
            initiating_address,
            entity,
            entity_named_keys,
            entity_access_rights,
            U512::MAX,
            Phase::Session,
        );

        let auction_method = request.auction_method();

        let result = match auction_method {
            AuctionMethod::ActivateBid {
                validator_public_key,
            } => runtime
                .activate_bid(validator_public_key)
                .map(|_| AuctionMethodRet::Unit)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
            AuctionMethod::AddBid {
                public_key,
                delegation_rate,
                amount,
            } => runtime
                .add_bid(public_key, delegation_rate, amount)
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(TrackingCopyError::Api),
            AuctionMethod::WithdrawBid { public_key, amount } => runtime
                .withdraw_bid(public_key, amount)
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
            AuctionMethod::Delegate {
                delegator_public_key,
                validator_public_key,
                amount,
                max_delegators_per_validator,
                minimum_delegation_amount,
            } => runtime
                .delegate(
                    delegator_public_key,
                    validator_public_key,
                    amount,
                    max_delegators_per_validator,
                    minimum_delegation_amount,
                )
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(TrackingCopyError::Api),
            AuctionMethod::Undelegate {
                delegator_public_key,
                validator_public_key,
                amount,
            } => runtime
                .undelegate(delegator_public_key, validator_public_key, amount)
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
            AuctionMethod::Redelegate {
                delegator_public_key,
                validator_public_key,
                amount,
                new_validator,
                minimum_delegation_amount,
            } => runtime
                .redelegate(
                    delegator_public_key,
                    validator_public_key,
                    amount,
                    new_validator,
                    minimum_delegation_amount,
                )
                .map(AuctionMethodRet::UpdatedAmount)
                .map_err(|auc_err| {
                    TrackingCopyError::SystemContract(system::Error::Auction(auc_err))
                }),
        };

        let effects = tc.borrow_mut().effects();

        // commit
        match result {
            Ok(ret) => match self.commit(state_hash, effects.clone()) {
                Ok(post_state_hash) => BiddingResult::Success {
                    ret,
                    post_state_hash,
                    effects,
                },
                Err(tce) => BiddingResult::Failure(tce.into()),
            },
            Err(tce) => BiddingResult::Failure(tce),
        }
    }
}

/// A trait expressing operations over the trie.
pub trait StateProvider {
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

    /// Balance inquiry.
    fn balance(&self, request: BalanceRequest) -> BalanceResult {
        let mut tc = match self.tracking_copy(request.state_hash()) {
            Ok(Some(tracking_copy)) => tracking_copy,
            Ok(None) => return BalanceResult::RootNotFound,
            Err(err) => return BalanceResult::Failure(TrackingCopyError::Storage(err)),
        };
        let protocol_version = request.protocol_version();
        let purse_uref = match request.identifier() {
            BalanceIdentifier::Purse(purse_uref) => *purse_uref,
            BalanceIdentifier::Public(public_key) => {
                let account_hash = public_key.to_account_hash();
                match tc.get_addressable_entity_by_account_hash(protocol_version, account_hash) {
                    Ok(entity) => entity.main_purse(),
                    Err(tce) => return BalanceResult::Failure(tce),
                }
            }
            BalanceIdentifier::Account(account_hash) => {
                match tc.get_addressable_entity_by_account_hash(protocol_version, *account_hash) {
                    Ok(entity) => entity.main_purse(),
                    Err(tce) => return BalanceResult::Failure(tce),
                }
            }
            BalanceIdentifier::Entity(entity_addr) => {
                match tc.get_addressable_entity(*entity_addr) {
                    Ok(entity) => entity.main_purse(),
                    Err(tce) => return BalanceResult::Failure(tce),
                }
            }
            BalanceIdentifier::Internal(addr) => casper_types::URef::new(*addr, AccessRights::READ),
        };
        let purse_key = purse_uref.into();
        // read the new hold records if any exist
        // check their timestamps..if stale tc.prune(that item)
        // total bal - sum(hold balance) == avail
        match tc.get_purse_balance_key(purse_key) {
            Ok(purse_balance_key) => match tc.get_purse_balance_with_proof(purse_balance_key) {
                Ok((balance, proof)) => {
                    let proof = Box::new(proof);
                    let motes = balance.value();
                    BalanceResult::Success { motes, proof }
                }
                Err(err) => BalanceResult::Failure(err),
            },
            Err(err) => BalanceResult::Failure(err),
        }
    }

    /// Get the requested era validators.
    fn era_validators(&self, request: EraValidatorsRequest) -> EraValidatorsResult {
        let state_hash = request.state_hash();
        let mut tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return EraValidatorsResult::RootNotFound,
            Err(err) => return EraValidatorsResult::Failure(TrackingCopyError::Storage(err)),
        };

        let query_request = match tc.get_system_entity_registry() {
            Ok(scr) => match scr.get(AUCTION).copied() {
                Some(auction_hash) => {
                    let key = if request.protocol_version().value().major < 2 {
                        Key::Hash(auction_hash.value())
                    } else {
                        Key::addressable_entity_key(EntityKindTag::System, auction_hash)
                    };
                    QueryRequest::new(
                        state_hash,
                        key,
                        vec![SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY.to_string()],
                    )
                }
                None => return EraValidatorsResult::AuctionNotFound,
            },
            Err(err) => return EraValidatorsResult::Failure(err),
        };

        let snapshot = match self.query(query_request) {
            QueryResult::RootNotFound => return EraValidatorsResult::RootNotFound,
            QueryResult::Failure(error) => {
                error!(?error, "unexpected tracking copy error");
                return EraValidatorsResult::Failure(error);
            }
            QueryResult::ValueNotFound(msg) => {
                error!(%msg, "value not found");
                return EraValidatorsResult::ValueNotFound(msg);
            }
            QueryResult::Success { value, proofs: _ } => {
                let cl_value = match value.into_cl_value() {
                    Some(snapshot_cl_value) => snapshot_cl_value,
                    None => {
                        error!("unexpected query failure; seigniorage recipients snapshot is not a CLValue");
                        return EraValidatorsResult::Failure(
                            TrackingCopyError::UnexpectedStoredValueVariant,
                        );
                    }
                };

                match cl_value.into_t() {
                    Ok(snapshot) => snapshot,
                    Err(cve) => {
                        return EraValidatorsResult::Failure(TrackingCopyError::CLValue(cve));
                    }
                }
            }
        };

        let era_validators = auction::detail::era_validators_from_snapshot(snapshot);
        EraValidatorsResult::Success { era_validators }
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
                        bids.push(bid_kind);
                    }
                    Some(_) => {
                        return BidsResult::Failure(TrackingCopyError::UnexpectedStoredValueVariant)
                    }
                    None => return BidsResult::Failure(TrackingCopyError::MissingBid(*key)),
                },
                Err(error) => return BidsResult::Failure(error),
            }
        }
        BidsResult::Success { bids }
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
                return ExecutionResultsChecksumResult::Failure(TrackingCopyError::Storage(err))
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
                        return AddressableEntityResult::ValueNotFound(msg)
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
                        return AddressableEntityResult::ValueNotFound(msg)
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
                ))
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
                        )
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
        let mut tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return SystemEntityRegistryResult::RootNotFound,
            Err(err) => {
                return SystemEntityRegistryResult::Failure(TrackingCopyError::Storage(err))
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
                    let key = if request.protocol_version().value().major < 2 {
                        Key::Hash(entity_hash.value())
                    } else {
                        Key::addressable_entity_key(EntityKindTag::System, entity_hash)
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

    /// Gets total supply.
    fn total_supply(&self, request: TotalSupplyRequest) -> TotalSupplyResult {
        let state_hash = request.state_hash();
        let mut tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return TotalSupplyResult::RootNotFound,
            Err(err) => return TotalSupplyResult::Failure(TrackingCopyError::Storage(err)),
        };

        let query_request = match tc.get_system_entity_registry() {
            Ok(scr) => match scr.get(MINT).copied() {
                Some(mint_hash) => {
                    let key = if request.protocol_version().value().major < 2 {
                        Key::Hash(mint_hash.value())
                    } else {
                        Key::addressable_entity_key(EntityKindTag::System, mint_hash)
                    };
                    QueryRequest::new(state_hash, key, vec![TOTAL_SUPPLY_KEY.to_string()])
                }
                None => {
                    error!("unexpected query failure; mint not found");
                    return TotalSupplyResult::MintNotFound;
                }
            },
            Err(err) => return TotalSupplyResult::Failure(err),
        };

        match self.query(query_request) {
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

    /// Gets the current round seigniorage rate.
    fn round_seigniorage_rate(
        &self,
        request: RoundSeigniorageRateRequest,
    ) -> RoundSeigniorageRateResult {
        let state_hash = request.state_hash();
        let mut tc = match self.tracking_copy(state_hash) {
            Ok(Some(tc)) => tc,
            Ok(None) => return RoundSeigniorageRateResult::RootNotFound,
            Err(err) => {
                return RoundSeigniorageRateResult::Failure(TrackingCopyError::Storage(err))
            }
        };

        let query_request = match tc.get_system_entity_registry() {
            Ok(scr) => match scr.get(MINT).copied() {
                Some(mint_hash) => {
                    let key = if request.protocol_version().value().major < 2 {
                        Key::Hash(mint_hash.value())
                    } else {
                        Key::addressable_entity_key(EntityKindTag::System, mint_hash)
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
            },
            Err(err) => return RoundSeigniorageRateResult::Failure(err),
        };

        match self.query(query_request) {
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
                    Err(cve) => {
                        RoundSeigniorageRateResult::Failure(TrackingCopyError::CLValue(cve))
                    }
                }
            }
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
                ))
            }
        };

        let source_account_hash = request.address();
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
            // if the source is not an admin, enforce_transfer_restrictions == true
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

        let (entity, entity_named_keys, entity_access_rights) =
            match tc.borrow_mut().resolved_entity(
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

        let id = crate::system::runtime_native::Id::Transaction(request.transaction_hash());
        // IMPORTANT: this runtime _must_ use the payer's context.
        let mut runtime = RuntimeNative::new(
            protocol_version,
            config.clone(),
            id,
            Rc::clone(&tc),
            source_account_hash,
            entity.clone(),
            entity_named_keys.clone(),
            entity_access_rights,
            remaining_spending_limit,
            Phase::Session,
        );

        match transfer_target_mode {
            NewTransferTargetMode::ExistingAccount { .. }
            | NewTransferTargetMode::PurseExists { .. } => {
                // Noop
            }
            NewTransferTargetMode::CreateAccount(account_hash) => {
                let main_purse = match runtime.mint(U512::zero()) {
                    Ok(uref) => uref,
                    Err(mint_error) => {
                        return TransferResult::Failure(TransferError::Mint(mint_error))
                    }
                };
                // TODO: KARAN TO FIX: this should create a shiny new addressable entity instance,
                // not create a legacy account and then uplift it.
                let account = Account::create(account_hash, NamedKeys::new(), main_purse);
                if let Err(tce) = tc
                    .borrow_mut()
                    .create_addressable_entity_from_account(account, protocol_version)
                {
                    return TransferResult::Failure(tce.into());
                }
            }
        }

        let transfer_args = {
            match runtime_args_builder.build(
                &entity,
                entity_named_keys,
                protocol_version,
                Rc::clone(&tc),
            ) {
                Ok(transfer_args) => transfer_args,
                Err(error) => return TransferResult::Failure(error),
            }
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

        {
            // TODO: this lexical block needs to be updated with the new versioned transaction types
            let deploy_hash = DeployHash::new(request.transaction_hash().digest());
            let deploy_info = casper_types::DeployInfo::new(
                deploy_hash,
                &transfers,
                source_account_hash,
                entity.main_purse(),
                request.cost(),
            );
            tc.borrow_mut().write(
                Key::DeployInfo(deploy_hash),
                StoredValue::DeployInfo(deploy_info),
            );
        }

        let effects = tc.borrow_mut().effects();

        TransferResult::Success { transfers, effects }
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

    /// Reads a `Trie` from the state if it is present
    fn trie(&self, request: TrieRequest) -> TrieResult;

    /// Persists a trie element.
    fn put_trie(&self, request: PutTrieRequest) -> PutTrieResult;

    /// Finds all the children of `trie_raw` which aren't present in the state.
    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, GlobalStateError>;
}

/// Write multiple key/stored value pairs to the store in a single rw transaction.
pub fn put_stored_values<'a, R, S, E>(
    environment: &'a R,
    store: &S,
    prestate_hash: Digest,
    stored_values: HashMap<Key, StoredValue>,
) -> Result<Digest, E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<Key, StoredValue>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error> + From<CommitError>,
{
    let mut txn = environment.create_read_write_txn()?;
    let mut state_root = prestate_hash;
    let maybe_root: Option<Trie<Key, StoredValue>> = store.get(&txn, &state_root)?;
    if maybe_root.is_none() {
        return Err(CommitError::RootNotFound(prestate_hash).into());
    };
    for (key, value) in stored_values.iter() {
        let write_result = write::<_, _, _, _, E>(&mut txn, store, &state_root, key, value)?;
        match write_result {
            WriteResult::Written(root_hash) => {
                state_root = root_hash;
            }
            WriteResult::AlreadyExists => (),
            WriteResult::RootNotFound => {
                error!(?state_root, ?key, ?value, "Error writing new value");
                return Err(CommitError::WriteRootNotFound(state_root).into());
            }
        }
    }
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
                debug!(?state_root, ?key, "commit: attempt to commit a read.");
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
