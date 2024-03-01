//! Global state.

/// Lmdb implementation of global state.
pub mod lmdb;

/// Lmdb implementation of global state with cache.
pub mod scratch;

use std::{cell::RefCell, collections::HashMap, convert::TryFrom, rc::Rc};

use tracing::{debug, error, warn};

use casper_types::{
    addressable_entity::{EntityKindTag, NamedKeys},
    bytesrepr,
    execution::{Effects, Transform, TransformError, TransformInstruction, TransformKind},
    global_state::TrieMerkleProof,
    system::{
        auction::SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
        mint::{ARG_AMOUNT, ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY},
        AUCTION, MINT,
    },
    Account, AddressableEntity, AddressableEntityHash, DeployHash, Digest, EntityAddr, Key, KeyTag,
    Phase, PublicKey, RuntimeArgs, StoredValue, U512,
};

#[cfg(test)]
pub use self::lmdb::make_temporary_global_state;

use crate::{
    data_access_layer::{
        era_validators::EraValidatorsResult,
        transfer::{TransferRequest, TransferRequestArgs, TransferResult},
        AddressableEntityRequest, AddressableEntityResult, BalanceRequest, BalanceResult,
        BidsRequest, BidsResult, EraValidatorsRequest, ExecutionResultsChecksumRequest,
        ExecutionResultsChecksumResult, FlushRequest, FlushResult, GenesisRequest, GenesisResult,
        ProtocolUpgradeRequest, ProtocolUpgradeResult, PutTrieRequest, PutTrieResult, QueryRequest,
        QueryResult, RoundSeigniorageRateRequest, RoundSeigniorageRateResult, TotalSupplyRequest,
        TotalSupplyResult, TrieRequest, TrieResult, EXECUTION_RESULTS_CHECKSUM_NAME,
    },
    global_state::{
        error::Error as GlobalStateError,
        transaction_source::{Transaction, TransactionSource},
        trie::Trie,
        trie_store::{
            operations::{prune, read, write, ReadResult, WriteResult},
            TrieStore,
        },
    },
    system::{
        auction,
        auction::bidding::{BiddingRequest, BiddingResult},
        genesis::{GenesisError, GenesisInstaller},
        mint::{Mint, NativeMintRuntime},
        protocol_upgrade::{ProtocolUpgradeError, ProtocolUpgrader},
        transfer::{NewTransferTargetMode, TransferError, TransferRuntimeArgsBuilder},
    },
    tracking_copy::{TrackingCopy, TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
};

use super::trie_store::operations::PruneResult;

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

/// Provides `commit` method.
pub trait CommitProvider: StateProvider {
    /// Applies changes and returns a new post state hash.
    /// block_hash is used for computing a deterministic and unique keys.
    fn commit(&self, state_hash: Digest, effects: Effects) -> Result<Digest, GlobalStateError>;

    fn genesis(&self, request: GenesisRequest) -> GenesisResult {
        let initial_root = self.empty_root();
        let tc = match self.tracking_copy(initial_root) {
            Ok(Some(tc)) => Rc::new(RefCell::new(tc)),
            Ok(None) => return GenesisResult::Fatal("state uninitialized".to_string()),
            Err(err) => {
                return GenesisResult::Failure(GenesisError::TrackingCopy(
                    crate::tracking_copy::TrackingCopyError::Storage(err),
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

        let account_hash = request.address();
        let protocol_version = request.protocol_version();
        if let Err(tce) = tc
            .borrow_mut()
            .migrate_account(account_hash, protocol_version)
        {
            return TransferResult::Failure(tce.into());
        }

        let authorization_keys = request.authorization_keys();

        let config = request.config();
        let administrative_accounts = config.administrative_accounts();

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

        if config.enforce_transfer_restrictions(&account_hash) {
            // We need to make sure that either the source or target is an admin.
            match transfer_target_mode {
                NewTransferTargetMode::ExistingAccount {
                    target_account_hash,
                    ..
                }
                | NewTransferTargetMode::CreateAccount(target_account_hash) => {
                    let is_target_system_account =
                        target_account_hash == PublicKey::System.to_account_hash();
                    let is_target_administrator = config.is_administrator(&target_account_hash);
                    if !(is_target_system_account || is_target_administrator) {
                        // Transferring from normal account to a purse doesn't work.
                        return TransferResult::Failure(
                            TransferError::DisabledUnrestrictedTransfers,
                        );
                    }
                }
                NewTransferTargetMode::PurseExists(_) => {
                    // We don't know who is the target and we can't simply reverse search
                    // account/contract that owns it. We also can't know if purse is owned exactly
                    // by one entity in the system.
                    return TransferResult::Failure(TransferError::DisabledUnrestrictedTransfers);
                }
            }
        }

        let (entity, entity_addr) = match tc.borrow_mut().get_authorized_addressable_entity(
            protocol_version,
            account_hash,
            authorization_keys,
            &administrative_accounts,
        ) {
            Ok((entity, entity_hash)) => {
                let entity_addr =
                    EntityAddr::new_with_tag(entity.entity_kind(), entity_hash.value());
                (entity, entity_addr)
            }
            Err(tce) => return TransferResult::Failure(TransferError::TrackingCopy(tce)),
        };

        let named_keys = match tc.borrow_mut().get_named_keys(entity_addr) {
            Ok(named_keys) => named_keys,
            Err(tce) => return TransferResult::Failure(TransferError::TrackingCopy(tce)),
        };
        let access_rights = entity
            .extract_access_rights(AddressableEntityHash::new(entity_addr.value()), &named_keys);

        let mut mint_provider = NativeMintRuntime::new(
            config.clone(),
            protocol_version,
            Rc::clone(&tc),
            account_hash,
            entity.clone(),
            named_keys.clone(),
            access_rights,
            remaining_spending_limit,
            request.transaction_hash(),
            Phase::Session,
        );

        match transfer_target_mode {
            NewTransferTargetMode::ExistingAccount { .. }
            | NewTransferTargetMode::PurseExists(_) => {
                // Noop
            }
            NewTransferTargetMode::CreateAccount(account_hash) => {
                let main_purse = match mint_provider.mint(U512::zero()) {
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
            match runtime_args_builder.build(&entity, named_keys, protocol_version, Rc::clone(&tc))
            {
                Ok(transfer_args) => transfer_args,
                Err(error) => return TransferResult::Failure(error),
            }
        };
        if let Err(mint_error) = mint_provider.transfer(
            transfer_args.to(),
            transfer_args.source(),
            transfer_args.target(),
            transfer_args.amount(),
            transfer_args.arg_id(),
        ) {
            return TransferResult::Failure(TransferError::Mint(mint_error));
        }

        let transfers = mint_provider.into_transfers();

        {
            // TODO: this block needs to be updated with version management for new style
            // Transactions
            let deploy_hash = DeployHash::new(request.transaction_hash());
            let deploy_info = casper_types::DeployInfo::new(
                deploy_hash,
                &transfers,
                account_hash,
                entity.main_purse(),
                request.cost(),
            );
            tc.borrow_mut().write(
                Key::DeployInfo(deploy_hash),
                StoredValue::DeployInfo(deploy_info),
            );
        }

        let effects = tc.borrow_mut().effects();

        // commit
        match self.commit(state_hash, effects.clone()) {
            Ok(post_state_hash) => TransferResult::Success {
                transfers,
                post_state_hash,
                effects,
            },
            Err(tce) => TransferResult::Failure(tce.into()),
        }
    }

    fn bidding(&self, _bid_request: BiddingRequest) -> BiddingResult {
        unimplemented!()
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

    /// Query balance state.
    fn balance(&self, request: BalanceRequest) -> BalanceResult {
        match self.tracking_copy(request.state_hash()) {
            Ok(Some(tracking_copy)) => {
                let purse_uref = request.purse_uref();
                let purse_key = purse_uref.into();
                // read the new hold records if any exist
                // check their timestamps..if stale tc.prune(that item)
                // total bal - sum(hold balance) == avail
                match tracking_copy.get_purse_balance_key(purse_key) {
                    Ok(purse_balance_key) => {
                        match tracking_copy.get_purse_balance_with_proof(purse_balance_key) {
                            Ok((balance, proof)) => {
                                let proof = Box::new(proof);
                                let motes = balance.value();
                                BalanceResult::Success { motes, proof }
                            }
                            Err(err) => BalanceResult::Failure(err),
                        }
                    }
                    Err(err) => BalanceResult::Failure(err),
                }
            }
            Ok(None) => BalanceResult::RootNotFound,
            Err(err) => BalanceResult::Failure(TrackingCopyError::Storage(err)),
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

    /// Reads a `Trie` from the state if it is present
    fn trie(&self, request: TrieRequest) -> TrieResult;

    /// Persists a trie element.
    fn put_trie(&self, request: PutTrieRequest) -> PutTrieResult;

    /// Finds all the children of `trie_raw` which aren't present in the state.
    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, GlobalStateError>;

    /// Prunes key from the global state.
    fn prune_keys(
        &self,
        root: Digest,
        keys_to_delete: &[Key],
    ) -> Result<PruneResult, GlobalStateError>;
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
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error> + From<CommitError>,
{
    let mut txn = environment.create_read_write_txn()?;
    let mut state_root = prestate_hash;

    let maybe_root: Option<Trie<Key, StoredValue>> = store.get(&txn, &state_root)?;

    if maybe_root.is_none() {
        return Err(CommitError::RootNotFound(prestate_hash).into());
    };

    for (key, kind) in effects.value().into_iter().map(Transform::destructure) {
        let read_result = read::<_, _, _, _, E>(&txn, store, &state_root, &key)?;

        let instruction = match (read_result, kind) {
            (_, TransformKind::Identity) => {
                // effectively a noop.
                debug!(?state_root, ?key, "commit: attempt to commit a read.");
                continue;
            }
            (ReadResult::NotFound, TransformKind::Write(new_value)) => {
                TransformInstruction::store(new_value)
            }
            (ReadResult::NotFound, TransformKind::Prune(key)) => {
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
                    PruneResult::Pruned(root_hash) => {
                        state_root = root_hash;
                    }
                    PruneResult::DoesNotExist => {
                        warn!("commit: pruning attempt failed for {}", key);
                    }
                    PruneResult::RootNotFound => {
                        error!(?state_root, ?key, "commit: root not found");
                        return Err(CommitError::WriteRootNotFound(state_root).into());
                    }
                }
            }
        }
    }

    txn.commit()?;

    Ok(state_root)
}
