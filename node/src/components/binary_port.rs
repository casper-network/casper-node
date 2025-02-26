//! The Binary Port
mod config;
mod connection_terminator;
mod error;
mod event;
mod metrics;
mod rate_limiter;
#[cfg(test)]
mod tests;

use std::{convert::TryFrom, net::SocketAddr, sync::Arc};

use casper_binary_port::{
    AccountInformation, AddressableEntityInformation, BalanceResponse, BinaryMessage,
    BinaryMessageCodec, BinaryResponse, BinaryResponseAndRequest, Command, CommandHeader,
    CommandTag, ContractInformation, DictionaryItemIdentifier, DictionaryQueryResult,
    EntityIdentifier, EraIdentifier, ErrorCode, GetRequest, GetTrieFullResult,
    GlobalStateEntityQualifier, GlobalStateQueryResult, GlobalStateRequest, InformationRequest,
    InformationRequestTag, KeyPrefix, NodeStatus, PackageIdentifier, PurseIdentifier,
    ReactorStateName, RecordId, ResponseType, RewardResponse, TransactionWithExecutionInfo,
    ValueWithProof,
};
use casper_storage::{
    data_access_layer::{
        balance::BalanceHandling,
        prefixed_values::{PrefixedValuesRequest, PrefixedValuesResult},
        tagged_values::{TaggedValuesRequest, TaggedValuesResult, TaggedValuesSelection},
        BalanceIdentifier, BalanceRequest, BalanceResult, ProofHandling, ProofsResult,
        QueryRequest, QueryResult, SeigniorageRecipientsRequest, SeigniorageRecipientsResult,
        TrieRequest,
    },
    global_state::trie::TrieRaw,
    system::auction,
    tracking_copy::TrackingCopyError,
    KeyPrefix as StorageKeyPrefix,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::NamedKeyAddr,
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    contracts::{ContractHash, ContractPackage, ContractPackageHash},
    BlockHeader, BlockIdentifier, BlockWithSignatures, ByteCode, ByteCodeAddr, ByteCodeHash,
    Chainspec, ContractWasm, ContractWasmHash, Digest, EntityAddr, GlobalStateIdentifier, Key,
    Package, PackageAddr, Peers, ProtocolVersion, Rewards, StoredValue, TimeDiff, Timestamp,
    Transaction, URef,
};
use connection_terminator::ConnectionTerminator;
use thiserror::Error as ThisError;

use datasize::DataSize;
use either::Either;
use futures::{SinkExt, StreamExt};
use once_cell::sync::OnceCell;
use rate_limiter::{LimiterResponse, RateLimiter, RateLimiterError};
use tokio::{
    join,
    net::{TcpListener, TcpStream},
    select,
    sync::{Mutex, Notify, OwnedSemaphorePermit, Semaphore},
};
use tokio_util::codec::{Encoder, Framed};
use tracing::{debug, error, info, trace, warn};

#[cfg(test)]
use futures::{future::BoxFuture, FutureExt};

use self::error::Error;
use crate::{
    contract_runtime::SpeculativeExecutionResult,
    effect::{
        requests::{
            AcceptTransactionRequest, BlockSynchronizerRequest, ChainspecRawBytesRequest,
            ConsensusRequest, ContractRuntimeRequest, NetworkInfoRequest, ReactorInfoRequest,
            StorageRequest, UpgradeWatcherRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    reactor::{main_reactor::MainEvent, QueueKind},
    types::NodeRng,
    utils::{display_error, ListeningError},
};
pub(crate) use metrics::Metrics;

use super::{Component, ComponentState, InitializedComponent, PortBoundComponent};

pub(crate) use config::Config;
pub(crate) use event::Event;

const COMPONENT_NAME: &str = "binary_port";

#[derive(Debug, ThisError)]
pub(crate) enum BinaryPortInitializationError {
    #[error("could not initialize rate limiter: {0}")]
    CannotInitializeRateLimiter(String),
    #[error("could not initialize metrics: {0}")]
    CannotInitializeMetrics(prometheus::Error),
}

impl From<RateLimiterError> for BinaryPortInitializationError {
    fn from(value: RateLimiterError) -> Self {
        BinaryPortInitializationError::CannotInitializeRateLimiter(value.to_string())
    }
}

impl From<prometheus::Error> for BinaryPortInitializationError {
    fn from(value: prometheus::Error) -> Self {
        BinaryPortInitializationError::CannotInitializeMetrics(value)
    }
}

#[derive(Debug, DataSize)]
pub(crate) struct BinaryPort {
    #[data_size(skip)]
    state: ComponentState,
    #[data_size(skip)]
    config: Arc<Config>,
    #[data_size(skip)]
    chainspec: Arc<Chainspec>,
    #[data_size(skip)]
    connection_limit: Arc<Semaphore>,
    #[data_size(skip)]
    metrics: Arc<Metrics>,
    #[data_size(skip)]
    local_addr: Arc<OnceCell<SocketAddr>>,
    #[data_size(skip)]
    shutdown_trigger: Arc<Notify>,
    #[data_size(skip)]
    server_join_handle: OnceCell<tokio::task::JoinHandle<()>>,
    #[data_size(skip)]
    rate_limiter: OnceCell<Arc<Mutex<RateLimiter>>>,
}

impl BinaryPort {
    pub(crate) fn new(config: Config, chainspec: Arc<Chainspec>, metrics: Metrics) -> Self {
        Self {
            state: ComponentState::Uninitialized,
            connection_limit: Arc::new(Semaphore::new(config.max_connections)),
            config: Arc::new(config),
            chainspec,
            metrics: Arc::new(metrics),
            local_addr: Arc::new(OnceCell::new()),
            shutdown_trigger: Arc::new(Notify::new()),
            server_join_handle: OnceCell::new(),
            rate_limiter: OnceCell::new(),
        }
    }

    /// Returns the binding address.
    ///
    /// Only used in testing.
    #[cfg(test)]
    pub(crate) fn bind_address(&self) -> Option<SocketAddr> {
        self.local_addr.get().cloned()
    }
}

struct BinaryRequestTerminationDelayValues {
    get_record: TimeDiff,
    get_information: TimeDiff,
    get_state: TimeDiff,
    get_trie: TimeDiff,
    accept_transaction: TimeDiff,
    speculative_exec: TimeDiff,
}

impl BinaryRequestTerminationDelayValues {
    fn from_config(config: &Config) -> Self {
        BinaryRequestTerminationDelayValues {
            get_record: config.get_record_request_termination_delay,
            get_information: config.get_information_request_termination_delay,
            get_state: config.get_state_request_termination_delay,
            get_trie: config.get_trie_request_termination_delay,
            accept_transaction: config.accept_transaction_request_termination_delay,
            speculative_exec: config.speculative_exec_request_termination_delay,
        }
    }
    fn get_life_termination_delay(&self, request: &Command) -> TimeDiff {
        match request {
            Command::Get(GetRequest::Record { .. }) => self.get_record,
            Command::Get(GetRequest::Information { .. }) => self.get_information,
            Command::Get(GetRequest::State(_)) => self.get_state,
            Command::Get(GetRequest::Trie { .. }) => self.get_trie,
            Command::TryAcceptTransaction { .. } => self.accept_transaction,
            Command::TrySpeculativeExec { .. } => self.speculative_exec,
        }
    }
}

async fn handle_request<REv>(
    req: Command,
    effect_builder: EffectBuilder<REv>,
    config: &Config,
    metrics: &Metrics,
    protocol_version: ProtocolVersion,
) -> BinaryResponse
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<AcceptTransactionRequest>
        + From<NetworkInfoRequest>
        + From<ReactorInfoRequest>
        + From<ConsensusRequest>
        + From<BlockSynchronizerRequest>
        + From<UpgradeWatcherRequest>
        + From<ChainspecRawBytesRequest>
        + Send,
{
    match req {
        Command::TryAcceptTransaction { transaction } => {
            metrics.binary_port_try_accept_transaction_count.inc();
            try_accept_transaction(effect_builder, transaction, false).await
        }
        Command::TrySpeculativeExec { transaction } => {
            metrics.binary_port_try_speculative_exec_count.inc();
            if !config.allow_request_speculative_exec {
                debug!(
                    hash = %transaction.hash(),
                    "received a request for speculative execution while the feature is disabled"
                );
                return BinaryResponse::new_error(ErrorCode::FunctionDisabled);
            }
            let response = try_accept_transaction(effect_builder, transaction.clone(), true).await;
            if !response.is_success() {
                return response;
            }
            try_speculative_execution(effect_builder, transaction).await
        }
        Command::Get(get_req) => {
            handle_get_request(get_req, effect_builder, config, metrics, protocol_version).await
        }
    }
}

async fn handle_get_request<REv>(
    get_req: GetRequest,
    effect_builder: EffectBuilder<REv>,
    config: &Config,
    metrics: &Metrics,
    protocol_version: ProtocolVersion,
) -> BinaryResponse
where
    REv: From<Event>
        + From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<ReactorInfoRequest>
        + From<ConsensusRequest>
        + From<BlockSynchronizerRequest>
        + From<UpgradeWatcherRequest>
        + From<ChainspecRawBytesRequest>
        + From<ContractRuntimeRequest>
        + Send,
{
    match get_req {
        // this workaround is in place because get_block_transfers performs a lazy migration
        GetRequest::Record {
            record_type_tag,
            key,
        } if RecordId::try_from(record_type_tag) == Ok(RecordId::Transfer) => {
            metrics.binary_port_get_record_count.inc();
            if key.is_empty() {
                return BinaryResponse::new_empty();
            }
            let Ok(block_hash) = bytesrepr::deserialize_from_slice(&key) else {
                debug!("received an incorrectly serialized key for a transfer record");
                return BinaryResponse::new_error(ErrorCode::TransferRecordMalformedKey);
            };
            let Some(transfers) = effect_builder
                .get_block_transfers_from_storage(block_hash)
                .await
            else {
                return BinaryResponse::new_empty();
            };
            let Ok(serialized) = bincode::serialize(&transfers) else {
                return BinaryResponse::new_error(ErrorCode::InternalError);
            };
            BinaryResponse::from_raw_bytes(ResponseType::Transfers, serialized)
        }
        GetRequest::Record {
            record_type_tag,
            key,
        } => {
            metrics.binary_port_get_record_count.inc();
            if key.is_empty() {
                return BinaryResponse::new_empty();
            }
            match RecordId::try_from(record_type_tag) {
                Ok(record_id) => {
                    let Some(db_bytes) = effect_builder.get_raw_data(record_id, key).await else {
                        return BinaryResponse::new_empty();
                    };
                    let payload_type =
                        ResponseType::from_record_id(record_id, db_bytes.is_legacy());
                    BinaryResponse::from_raw_bytes(payload_type, db_bytes.into_raw_bytes())
                }
                Err(_) => BinaryResponse::new_error(ErrorCode::UnsupportedRequest),
            }
        }
        GetRequest::Information { info_type_tag, key } => {
            metrics.binary_port_get_info_count.inc();
            let Ok(tag) = InformationRequestTag::try_from(info_type_tag) else {
                debug!(
                    tag = info_type_tag,
                    "received an unknown information request tag"
                );
                return BinaryResponse::new_error(ErrorCode::UnsupportedRequest);
            };
            match InformationRequest::try_from((tag, &key[..])) {
                Ok(req) => handle_info_request(req, effect_builder, protocol_version).await,
                Err(error) => {
                    debug!(?tag, %error, "failed to parse an information request");
                    BinaryResponse::new_error(ErrorCode::MalformedInformationRequest)
                }
            }
        }
        GetRequest::State(req) => {
            metrics.binary_port_get_state_count.inc();
            handle_state_request(effect_builder, *req, protocol_version, config).await
        }
        GetRequest::Trie { trie_key } => {
            metrics.binary_port_get_trie_count.inc();
            handle_trie_request(effect_builder, trie_key, config).await
        }
    }
}

async fn handle_get_items_by_prefix<REv>(
    state_identifier: Option<GlobalStateIdentifier>,
    key_prefix: KeyPrefix,
    effect_builder: EffectBuilder<REv>,
) -> BinaryResponse
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    let Some(state_root_hash) = resolve_state_root_hash(effect_builder, state_identifier).await
    else {
        return BinaryResponse::new_error(ErrorCode::RootNotFound);
    };
    let storage_key_prefix = match key_prefix {
        KeyPrefix::DelegatorBidAddrsByValidator(hash) => {
            StorageKeyPrefix::DelegatorBidAddrsByValidator(hash)
        }
        KeyPrefix::MessagesByEntity(addr) => StorageKeyPrefix::MessageEntriesByEntity(addr),
        KeyPrefix::MessagesByEntityAndTopic(addr, topic) => {
            StorageKeyPrefix::MessagesByEntityAndTopic(addr, topic)
        }
        KeyPrefix::NamedKeysByEntity(addr) => StorageKeyPrefix::NamedKeysByEntity(addr),
        KeyPrefix::GasBalanceHoldsByPurse(purse) => StorageKeyPrefix::GasBalanceHoldsByPurse(purse),
        KeyPrefix::ProcessingBalanceHoldsByPurse(purse) => {
            StorageKeyPrefix::ProcessingBalanceHoldsByPurse(purse)
        }
        KeyPrefix::EntryPointsV1ByEntity(addr) => StorageKeyPrefix::EntryPointsV1ByEntity(addr),
        KeyPrefix::EntryPointsV2ByEntity(addr) => StorageKeyPrefix::EntryPointsV2ByEntity(addr),
    };
    let request = PrefixedValuesRequest::new(state_root_hash, storage_key_prefix);
    match effect_builder.get_prefixed_values(request).await {
        PrefixedValuesResult::Success { values, .. } => BinaryResponse::from_value(values),
        PrefixedValuesResult::RootNotFound => BinaryResponse::new_error(ErrorCode::RootNotFound),
        PrefixedValuesResult::Failure(error) => {
            debug!(%error, "failed when querying for values by prefix");
            BinaryResponse::new_error(ErrorCode::InternalError)
        }
    }
}

async fn handle_get_all_items<REv>(
    state_identifier: Option<GlobalStateIdentifier>,
    key_tag: casper_types::KeyTag,
    effect_builder: EffectBuilder<REv>,
) -> BinaryResponse
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    let Some(state_root_hash) = resolve_state_root_hash(effect_builder, state_identifier).await
    else {
        return BinaryResponse::new_error(ErrorCode::RootNotFound);
    };
    let request = TaggedValuesRequest::new(state_root_hash, TaggedValuesSelection::All(key_tag));
    match effect_builder.get_tagged_values(request).await {
        TaggedValuesResult::Success { values, .. } => BinaryResponse::from_value(values),
        TaggedValuesResult::RootNotFound => BinaryResponse::new_error(ErrorCode::RootNotFound),
        TaggedValuesResult::Failure(error) => {
            debug!(%error, "failed when querying for all values by tag");
            BinaryResponse::new_error(ErrorCode::InternalError)
        }
    }
}

async fn handle_state_request<REv>(
    effect_builder: EffectBuilder<REv>,
    request: GlobalStateRequest,
    protocol_version: ProtocolVersion,
    config: &Config,
) -> BinaryResponse
where
    REv: From<Event>
        + From<ContractRuntimeRequest>
        + From<StorageRequest>
        + From<ReactorInfoRequest>,
{
    let (state_identifier, qualifier) = request.destructure();
    match qualifier {
        GlobalStateEntityQualifier::Item { base_key, path } => {
            let Some(state_root_hash) =
                resolve_state_root_hash(effect_builder, state_identifier).await
            else {
                return BinaryResponse::new_error(ErrorCode::RootNotFound);
            };
            match get_global_state_item(effect_builder, state_root_hash, base_key, path).await {
                Ok(Some(result)) => BinaryResponse::from_value(result),
                Ok(None) => BinaryResponse::new_empty(),
                Err(err) => BinaryResponse::new_error(err),
            }
        }
        GlobalStateEntityQualifier::AllItems { key_tag } => {
            if !config.allow_request_get_all_values {
                debug!(%key_tag, "received a request for items by key tag while the feature is disabled");
                BinaryResponse::new_error(ErrorCode::FunctionDisabled)
            } else {
                handle_get_all_items(state_identifier, key_tag, effect_builder).await
            }
        }
        GlobalStateEntityQualifier::DictionaryItem { identifier } => {
            let Some(state_root_hash) =
                resolve_state_root_hash(effect_builder, state_identifier).await
            else {
                return BinaryResponse::new_error(ErrorCode::RootNotFound);
            };
            let result = match identifier {
                DictionaryItemIdentifier::AccountNamedKey {
                    hash,
                    dictionary_name,
                    dictionary_item_key,
                } => {
                    get_dictionary_item_by_legacy_named_key(
                        effect_builder,
                        state_root_hash,
                        Key::Account(hash),
                        dictionary_name,
                        dictionary_item_key,
                    )
                    .await
                }
                DictionaryItemIdentifier::ContractNamedKey {
                    hash,
                    dictionary_name,
                    dictionary_item_key,
                } => {
                    get_dictionary_item_by_legacy_named_key(
                        effect_builder,
                        state_root_hash,
                        Key::Hash(hash),
                        dictionary_name,
                        dictionary_item_key,
                    )
                    .await
                }
                DictionaryItemIdentifier::EntityNamedKey {
                    addr,
                    dictionary_name,
                    dictionary_item_key,
                } => {
                    get_dictionary_item_by_named_key(
                        effect_builder,
                        state_root_hash,
                        addr,
                        dictionary_name,
                        dictionary_item_key,
                    )
                    .await
                }
                DictionaryItemIdentifier::URef {
                    seed_uref,
                    dictionary_item_key,
                } => {
                    let key = Key::dictionary(seed_uref, dictionary_item_key.as_bytes());
                    get_global_state_item(effect_builder, state_root_hash, key, vec![])
                        .await
                        .map(|maybe_res| maybe_res.map(|res| DictionaryQueryResult::new(key, res)))
                }
                DictionaryItemIdentifier::DictionaryItem(addr) => {
                    let key = Key::Dictionary(addr);
                    get_global_state_item(effect_builder, state_root_hash, key, vec![])
                        .await
                        .map(|maybe_res| maybe_res.map(|res| DictionaryQueryResult::new(key, res)))
                }
            };
            match result {
                Ok(Some(result)) => BinaryResponse::from_value(result),
                Ok(None) => BinaryResponse::new_empty(),
                Err(err) => BinaryResponse::new_error(err),
            }
        }
        GlobalStateEntityQualifier::Balance { purse_identifier } => {
            let Some(state_root_hash) =
                resolve_state_root_hash(effect_builder, state_identifier).await
            else {
                return BinaryResponse::new_empty();
            };
            get_balance(
                effect_builder,
                state_root_hash,
                purse_identifier,
                protocol_version,
            )
            .await
        }
        GlobalStateEntityQualifier::ItemsByPrefix { key_prefix } => {
            handle_get_items_by_prefix(state_identifier, key_prefix, effect_builder).await
        }
    }
}

async fn handle_trie_request<REv>(
    effect_builder: EffectBuilder<REv>,
    trie_key: Digest,
    config: &Config,
) -> BinaryResponse
where
    REv: From<Event>
        + From<ContractRuntimeRequest>
        + From<StorageRequest>
        + From<ReactorInfoRequest>,
{
    if !config.allow_request_get_trie {
        debug!(%trie_key, "received a trie request while the feature is disabled");
        BinaryResponse::new_error(ErrorCode::FunctionDisabled)
    } else {
        let req = TrieRequest::new(trie_key, None);
        match effect_builder.get_trie(req).await.into_raw() {
            Ok(result) => {
                BinaryResponse::from_value(GetTrieFullResult::new(result.map(TrieRaw::into_inner)))
            }
            Err(error) => {
                debug!(%error, "failed when querying for a trie");
                BinaryResponse::new_error(ErrorCode::InternalError)
            }
        }
    }
}

async fn get_dictionary_item_by_legacy_named_key<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    entity_key: Key,
    dictionary_name: String,
    dictionary_item_key: String,
) -> Result<Option<DictionaryQueryResult>, ErrorCode>
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    match effect_builder
        .query_global_state(QueryRequest::new(state_root_hash, entity_key, vec![]))
        .await
    {
        QueryResult::Success { value, .. } => {
            let named_keys = match &*value {
                StoredValue::Account(account) => account.named_keys(),
                StoredValue::Contract(contract) => contract.named_keys(),
                value => {
                    debug!(
                        value_type = value.type_name(),
                        "unexpected stored value found when querying for a dictionary"
                    );
                    return Err(ErrorCode::DictionaryURefNotFound);
                }
            };
            let Some(uref) = named_keys.get(&dictionary_name).and_then(Key::as_uref) else {
                debug!(
                    dictionary_name,
                    "dictionary seed URef not found in named keys"
                );
                return Err(ErrorCode::DictionaryURefNotFound);
            };
            let key = Key::dictionary(*uref, dictionary_item_key.as_bytes());
            let Some(query_result) =
                get_global_state_item(effect_builder, state_root_hash, key, vec![]).await?
            else {
                return Ok(None);
            };

            Ok(Some(DictionaryQueryResult::new(key, query_result)))
        }
        QueryResult::RootNotFound => {
            debug!("root not found when querying for a dictionary seed URef");
            Err(ErrorCode::DictionaryURefNotFound)
        }
        QueryResult::ValueNotFound(error) => {
            debug!(%error, "value not found when querying for a dictionary seed URef");
            Err(ErrorCode::DictionaryURefNotFound)
        }
        QueryResult::Failure(error) => {
            debug!(%error, "failed when querying for a dictionary seed URef");
            Err(ErrorCode::FailedQuery)
        }
    }
}

async fn get_dictionary_item_by_named_key<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    entity_addr: EntityAddr,
    dictionary_name: String,
    dictionary_item_key: String,
) -> Result<Option<DictionaryQueryResult>, ErrorCode>
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    let Ok(key_addr) = NamedKeyAddr::new_from_string(entity_addr, dictionary_name) else {
        return Err(ErrorCode::InternalError);
    };
    let req = QueryRequest::new(state_root_hash, Key::NamedKey(key_addr), vec![]);
    match effect_builder.query_global_state(req).await {
        QueryResult::Success { value, .. } => {
            let key_val = match &*value {
                StoredValue::NamedKey(key_val) => key_val,
                value => {
                    debug!(
                        value_type = value.type_name(),
                        "unexpected stored value found when querying for a dictionary"
                    );
                    return Err(ErrorCode::DictionaryURefNotFound);
                }
            };
            let uref = match key_val.get_key() {
                Ok(Key::URef(uref)) => uref,
                result => {
                    debug!(
                        ?result,
                        "unexpected named key result when querying for a dictionary"
                    );
                    return Err(ErrorCode::DictionaryURefNotFound);
                }
            };
            let key = Key::dictionary(uref, dictionary_item_key.as_bytes());
            let Some(query_result) =
                get_global_state_item(effect_builder, state_root_hash, key, vec![]).await?
            else {
                return Ok(None);
            };
            Ok(Some(DictionaryQueryResult::new(key, query_result)))
        }
        QueryResult::RootNotFound => {
            debug!("root not found when querying for a dictionary seed URef");
            Err(ErrorCode::DictionaryURefNotFound)
        }
        QueryResult::ValueNotFound(error) => {
            debug!(%error, "value not found when querying for a dictionary seed URef");
            Err(ErrorCode::DictionaryURefNotFound)
        }
        QueryResult::Failure(error) => {
            debug!(%error, "failed when querying for a dictionary seed URef");
            Err(ErrorCode::FailedQuery)
        }
    }
}

async fn get_balance<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    purse_identifier: PurseIdentifier,
    protocol_version: ProtocolVersion,
) -> BinaryResponse
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ReactorInfoRequest>,
{
    let balance_id = match purse_identifier {
        PurseIdentifier::Payment => BalanceIdentifier::Payment,
        PurseIdentifier::Accumulate => BalanceIdentifier::Accumulate,
        PurseIdentifier::Purse(uref) => BalanceIdentifier::Purse(uref),
        PurseIdentifier::PublicKey(pub_key) => BalanceIdentifier::Public(pub_key),
        PurseIdentifier::Account(account) => BalanceIdentifier::Account(account),
        PurseIdentifier::Entity(entity) => BalanceIdentifier::Entity(entity),
    };
    let balance_handling = BalanceHandling::Available;

    let balance_req = BalanceRequest::new(
        state_root_hash,
        protocol_version,
        balance_id,
        balance_handling,
        ProofHandling::Proofs,
    );
    match effect_builder.get_balance(balance_req).await {
        BalanceResult::RootNotFound => BinaryResponse::new_error(ErrorCode::RootNotFound),
        BalanceResult::Success {
            total_balance,
            available_balance,
            proofs_result,
            ..
        } => {
            let ProofsResult::Proofs {
                total_balance_proof,
                balance_holds,
            } = proofs_result
            else {
                warn!("binary port received no proofs for a balance request with proofs");
                return BinaryResponse::new_error(ErrorCode::InternalError);
            };
            let response = BalanceResponse {
                total_balance,
                available_balance,
                total_balance_proof,
                balance_holds,
            };
            BinaryResponse::from_value(response)
        }
        BalanceResult::Failure(TrackingCopyError::KeyNotFound(_)) => {
            BinaryResponse::new_error(ErrorCode::PurseNotFound)
        }
        BalanceResult::Failure(error) => {
            debug!(%error, "failed when querying for a balance");
            BinaryResponse::new_error(ErrorCode::FailedQuery)
        }
    }
}

async fn get_global_state_item<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    base_key: Key,
    path: Vec<String>,
) -> Result<Option<GlobalStateQueryResult>, ErrorCode>
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    match effect_builder
        .query_global_state(QueryRequest::new(state_root_hash, base_key, path))
        .await
    {
        QueryResult::Success { value, proofs } => {
            Ok(Some(GlobalStateQueryResult::new(*value, proofs)))
        }
        QueryResult::RootNotFound => Err(ErrorCode::RootNotFound),
        QueryResult::ValueNotFound(error) => {
            debug!(%error, "value not found when querying for a global state item");
            Err(ErrorCode::NotFound)
        }
        QueryResult::Failure(error) => {
            debug!(%error, "failed when querying for a global state item");
            Err(ErrorCode::FailedQuery)
        }
    }
}

async fn get_contract_package<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    hash: ContractPackageHash,
) -> Result<Option<Either<ValueWithProof<ContractPackage>, ValueWithProof<Package>>>, ErrorCode>
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ReactorInfoRequest>,
{
    let key = Key::Hash(hash.value());
    let Some(result) = get_global_state_item(effect_builder, state_root_hash, key, vec![]).await?
    else {
        return Ok(None);
    };
    match result.into_inner() {
        (StoredValue::ContractPackage(contract), proof) => {
            Ok(Some(Either::Left(ValueWithProof::new(contract, proof))))
        }
        (other, _) => {
            let Some((Key::SmartContract(addr), _)) = other
                .as_cl_value()
                .and_then(|cl_val| cl_val.to_t::<(Key, URef)>().ok())
            else {
                debug!(
                    ?other,
                    "unexpected stored value found when querying for a contract package"
                );
                return Err(ErrorCode::InternalError);
            };
            let package = get_package(effect_builder, state_root_hash, addr).await?;
            Ok(package.map(Either::Right))
        }
    }
}

async fn get_package<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    package_addr: PackageAddr,
) -> Result<Option<ValueWithProof<Package>>, ErrorCode>
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ReactorInfoRequest>,
{
    let key = Key::SmartContract(package_addr);
    let Some(result) = get_global_state_item(effect_builder, state_root_hash, key, vec![]).await?
    else {
        return Ok(None);
    };
    match result.into_inner() {
        (StoredValue::SmartContract(contract), proof) => {
            Ok(Some(ValueWithProof::new(contract, proof)))
        }
        other => {
            debug!(
                ?other,
                "unexpected stored value found when querying for a package"
            );
            Err(ErrorCode::InternalError)
        }
    }
}

async fn get_contract<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    hash: ContractHash,
    include_wasm: bool,
) -> Result<Option<Either<ContractInformation, AddressableEntityInformation>>, ErrorCode>
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ReactorInfoRequest>,
{
    let key = Key::Hash(hash.value());
    let Some(result) = get_global_state_item(effect_builder, state_root_hash, key, vec![]).await?
    else {
        return Ok(None);
    };
    match result.into_inner() {
        (StoredValue::Contract(contract), proof)
            if include_wasm && contract.contract_wasm_hash() != ContractWasmHash::default() =>
        {
            let wasm_hash = contract.contract_wasm_hash();
            let Some(wasm) = get_contract_wasm(effect_builder, state_root_hash, wasm_hash).await?
            else {
                return Ok(None);
            };
            Ok(Some(Either::Left(ContractInformation::new(
                hash,
                ValueWithProof::new(contract, proof),
                Some(wasm),
            ))))
        }
        (StoredValue::Contract(contract), proof) => Ok(Some(Either::Left(
            ContractInformation::new(hash, ValueWithProof::new(contract, proof), None),
        ))),
        (other, _) => {
            let Some(Key::AddressableEntity(addr)) = other
                .as_cl_value()
                .and_then(|cl_val| cl_val.to_t::<Key>().ok())
            else {
                debug!(
                    ?other,
                    "unexpected stored value found when querying for a contract"
                );
                return Err(ErrorCode::InternalError);
            };
            let entity = get_entity(effect_builder, state_root_hash, addr, include_wasm).await?;
            Ok(entity.map(Either::Right))
        }
    }
}

async fn get_account<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    hash: AccountHash,
    include_bytecode: bool,
) -> Result<Option<Either<AccountInformation, AddressableEntityInformation>>, ErrorCode>
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ReactorInfoRequest>,
{
    let key = Key::Account(hash);
    let Some(result) = get_global_state_item(effect_builder, state_root_hash, key, vec![]).await?
    else {
        return Ok(None);
    };
    match result.into_inner() {
        (StoredValue::Account(account), proof) => {
            Ok(Some(Either::Left(AccountInformation::new(account, proof))))
        }
        (other, _) => {
            let Some(Key::AddressableEntity(addr)) = other
                .as_cl_value()
                .and_then(|cl_val| cl_val.to_t::<Key>().ok())
            else {
                debug!(
                    ?other,
                    "unexpected stored value found when querying for an account"
                );
                return Err(ErrorCode::InternalError);
            };
            let entity =
                get_entity(effect_builder, state_root_hash, addr, include_bytecode).await?;
            Ok(entity.map(Either::Right))
        }
    }
}

async fn get_entity<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    addr: EntityAddr,
    include_bytecode: bool,
) -> Result<Option<AddressableEntityInformation>, ErrorCode>
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ReactorInfoRequest>,
{
    let key = Key::from(addr);
    let Some(result) = get_global_state_item(effect_builder, state_root_hash, key, vec![]).await?
    else {
        return Ok(None);
    };
    match result.into_inner() {
        (StoredValue::AddressableEntity(entity), proof)
            if include_bytecode && entity.byte_code_hash() != ByteCodeHash::default() =>
        {
            let Some(bytecode) =
                get_contract_bytecode(effect_builder, state_root_hash, entity.byte_code_hash())
                    .await?
            else {
                return Ok(None);
            };
            Ok(Some(AddressableEntityInformation::new(
                addr,
                ValueWithProof::new(entity, proof),
                Some(bytecode),
            )))
        }
        (StoredValue::AddressableEntity(entity), proof) => Ok(Some(
            AddressableEntityInformation::new(addr, ValueWithProof::new(entity, proof), None),
        )),
        (other, _) => {
            debug!(
                ?other,
                "unexpected stored value found when querying for an entity"
            );
            Err(ErrorCode::InternalError)
        }
    }
}

async fn get_contract_wasm<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    hash: ContractWasmHash,
) -> Result<Option<ValueWithProof<ContractWasm>>, ErrorCode>
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ReactorInfoRequest>,
{
    let key = Key::from(hash);
    let Some(value) = get_global_state_item(effect_builder, state_root_hash, key, vec![]).await?
    else {
        return Ok(None);
    };
    match value.into_inner() {
        (StoredValue::ContractWasm(wasm), proof) => Ok(Some(ValueWithProof::new(wasm, proof))),
        other => {
            debug!(
                ?other,
                "unexpected stored value found when querying for Wasm"
            );
            Err(ErrorCode::InternalError)
        }
    }
}

async fn get_contract_bytecode<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    addr: ByteCodeHash,
) -> Result<Option<ValueWithProof<ByteCode>>, ErrorCode>
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ReactorInfoRequest>,
{
    let key = Key::ByteCode(ByteCodeAddr::new_wasm_addr(addr.value()));
    let Some(value) = get_global_state_item(effect_builder, state_root_hash, key, vec![]).await?
    else {
        return Ok(None);
    };
    match value.into_inner() {
        (StoredValue::ByteCode(bytecode), proof) => Ok(Some(ValueWithProof::new(bytecode, proof))),
        other => {
            debug!(
                ?other,
                "unexpected stored value found when querying for bytecode"
            );
            Err(ErrorCode::InternalError)
        }
    }
}

async fn handle_info_request<REv>(
    req: InformationRequest,
    effect_builder: EffectBuilder<REv>,
    protocol_version: ProtocolVersion,
) -> BinaryResponse
where
    REv: From<Event>
        + From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<ReactorInfoRequest>
        + From<ConsensusRequest>
        + From<BlockSynchronizerRequest>
        + From<UpgradeWatcherRequest>
        + From<ChainspecRawBytesRequest>
        + From<ContractRuntimeRequest>
        + Send,
{
    match req {
        InformationRequest::BlockHeader(identifier) => {
            let maybe_header = resolve_block_header(effect_builder, identifier).await;
            BinaryResponse::from_option(maybe_header)
        }
        InformationRequest::BlockWithSignatures(identifier) => {
            let Some(height) = resolve_block_height(effect_builder, identifier).await else {
                return BinaryResponse::new_empty();
            };
            let Some(block) = effect_builder
                .get_block_at_height_with_metadata_from_storage(height, true)
                .await
            else {
                return BinaryResponse::new_empty();
            };
            BinaryResponse::from_value(BlockWithSignatures::new(
                block.block,
                block.block_signatures,
            ))
        }
        InformationRequest::Transaction {
            hash,
            with_finalized_approvals,
        } => {
            let Some((transaction, execution_info)) = effect_builder
                .get_transaction_and_exec_info_from_storage(hash, with_finalized_approvals)
                .await
            else {
                return BinaryResponse::new_empty();
            };
            BinaryResponse::from_value(TransactionWithExecutionInfo::new(
                transaction,
                execution_info,
            ))
        }
        InformationRequest::Peers => {
            BinaryResponse::from_value(Peers::from(effect_builder.network_peers().await))
        }
        InformationRequest::Uptime => BinaryResponse::from_value(effect_builder.get_uptime().await),
        InformationRequest::LastProgress => {
            BinaryResponse::from_value(effect_builder.get_last_progress().await)
        }
        InformationRequest::ReactorState => {
            let state = effect_builder.get_reactor_state().await;
            BinaryResponse::from_value(ReactorStateName::new(state))
        }
        InformationRequest::NetworkName => {
            BinaryResponse::from_value(effect_builder.get_network_name().await)
        }
        InformationRequest::ConsensusValidatorChanges => {
            BinaryResponse::from_value(effect_builder.get_consensus_validator_changes().await)
        }
        InformationRequest::BlockSynchronizerStatus => {
            BinaryResponse::from_value(effect_builder.get_block_synchronizer_status().await)
        }
        InformationRequest::AvailableBlockRange => BinaryResponse::from_value(
            effect_builder
                .get_available_block_range_from_storage()
                .await,
        ),
        InformationRequest::NextUpgrade => {
            BinaryResponse::from_option(effect_builder.get_next_upgrade().await)
        }
        InformationRequest::ConsensusStatus => {
            BinaryResponse::from_option(effect_builder.consensus_status().await)
        }
        InformationRequest::ChainspecRawBytes => {
            BinaryResponse::from_value((*effect_builder.get_chainspec_raw_bytes().await).clone())
        }
        InformationRequest::LatestSwitchBlockHeader => BinaryResponse::from_option(
            effect_builder
                .get_latest_switch_block_header_from_storage()
                .await,
        ),
        InformationRequest::NodeStatus => {
            let (
                node_uptime,
                network_name,
                last_added_block,
                peers,
                next_upgrade,
                consensus_status,
                reactor_state,
                last_progress,
                available_block_range,
                block_sync,
                latest_switch_block_header,
            ) = join!(
                effect_builder.get_uptime(),
                effect_builder.get_network_name(),
                effect_builder.get_highest_complete_block_from_storage(),
                effect_builder.network_peers(),
                effect_builder.get_next_upgrade(),
                effect_builder.consensus_status(),
                effect_builder.get_reactor_state(),
                effect_builder.get_last_progress(),
                effect_builder.get_available_block_range_from_storage(),
                effect_builder.get_block_synchronizer_status(),
                effect_builder.get_latest_switch_block_header_from_storage(),
            );
            let starting_state_root_hash = effect_builder
                .get_block_header_at_height_from_storage(available_block_range.low(), true)
                .await
                .map(|header| *header.state_root_hash())
                .unwrap_or_default();
            let (our_public_signing_key, round_length) =
                consensus_status.map_or((None, None), |consensus_status| {
                    (
                        Some(consensus_status.validator_public_key().clone()),
                        consensus_status.round_length(),
                    )
                });
            let reactor_state = ReactorStateName::new(reactor_state);

            let Ok(uptime) = TimeDiff::try_from(node_uptime) else {
                return BinaryResponse::new_error(ErrorCode::InternalError);
            };

            let status = NodeStatus {
                protocol_version,
                peers: Peers::from(peers),
                build_version: crate::VERSION_STRING.clone(),
                chainspec_name: network_name.into(),
                starting_state_root_hash,
                last_added_block_info: last_added_block.map(Into::into),
                our_public_signing_key,
                round_length,
                next_upgrade,
                uptime,
                reactor_state,
                last_progress: last_progress.into(),
                available_block_range,
                block_sync,
                latest_switch_block_hash: latest_switch_block_header
                    .map(|header| header.block_hash()),
            };
            BinaryResponse::from_value(status)
        }
        InformationRequest::Reward {
            era_identifier,
            validator,
            delegator,
        } => {
            let Some(header) =
                resolve_era_switch_block_header(effect_builder, era_identifier).await
            else {
                return BinaryResponse::new_error(ErrorCode::SwitchBlockNotFound);
            };
            let Some(previous_height) = header.height().checked_sub(1) else {
                // there's not going to be any rewards for the genesis block
                debug!("received a request for rewards in the genesis block");
                return BinaryResponse::new_empty();
            };
            let Some(parent_header) = effect_builder
                .get_block_header_at_height_from_storage(previous_height, true)
                .await
            else {
                return BinaryResponse::new_error(ErrorCode::SwitchBlockParentNotFound);
            };
            let snapshot_request =
                SeigniorageRecipientsRequest::new(*parent_header.state_root_hash());

            let snapshot = match effect_builder
                .get_seigniorage_recipients_snapshot_from_contract_runtime(snapshot_request)
                .await
            {
                SeigniorageRecipientsResult::Success {
                    seigniorage_recipients,
                } => seigniorage_recipients,
                SeigniorageRecipientsResult::RootNotFound => {
                    return BinaryResponse::new_error(ErrorCode::RootNotFound)
                }
                SeigniorageRecipientsResult::Failure(error) => {
                    warn!(%error, "failed when querying for seigniorage recipients");
                    return BinaryResponse::new_error(ErrorCode::FailedQuery);
                }
                SeigniorageRecipientsResult::AuctionNotFound => {
                    warn!("auction not found when querying for seigniorage recipients");
                    return BinaryResponse::new_error(ErrorCode::InternalError);
                }
                SeigniorageRecipientsResult::ValueNotFound(error) => {
                    warn!(%error, "value not found when querying for seigniorage recipients");
                    return BinaryResponse::new_error(ErrorCode::InternalError);
                }
            };
            let Some(era_end) = header.clone_era_end() else {
                // switch block should have an era end
                error!(
                    hash = %header.block_hash(),
                    "switch block missing era end (undefined behavior)"
                );
                return BinaryResponse::new_error(ErrorCode::InternalError);
            };
            let block_rewards = match era_end.rewards() {
                Rewards::V2(rewards) => rewards,
                Rewards::V1(_) => {
                    //It is possible to calculate V1 rewards, but previously we didn't support an
                    // endpoint to report it in that way. We could implement it
                    // in a future release if there is interest in it - it's not trivial though.
                    return BinaryResponse::new_error(ErrorCode::UnsupportedRewardsV1Request);
                }
            };
            let Some(validator_rewards) = block_rewards.get(&validator) else {
                return BinaryResponse::new_empty();
            };

            let seigniorage_recipient =
                snapshot.get_seignorage_recipient(&header.era_id(), &validator);

            let reward = auction::reward(
                &validator,
                delegator.as_deref(),
                header.era_id(),
                validator_rewards,
                &snapshot,
            );
            match (reward, seigniorage_recipient) {
                (Ok(Some(reward)), Some(seigniorage_recipient)) => {
                    let response = RewardResponse::new(
                        reward,
                        header.era_id(),
                        *seigniorage_recipient.delegation_rate(),
                        header.block_hash(),
                    );
                    BinaryResponse::from_value(response)
                }
                (Err(error), _) => {
                    warn!(%error, "failed when calculating rewards");
                    BinaryResponse::new_error(ErrorCode::InternalError)
                }
                _ => BinaryResponse::new_empty(),
            }
        }
        InformationRequest::ProtocolVersion => BinaryResponse::from_value(protocol_version),
        InformationRequest::Package {
            state_identifier,
            identifier,
        } => {
            let Some(state_root_hash) =
                resolve_state_root_hash(effect_builder, state_identifier).await
            else {
                return BinaryResponse::new_error(ErrorCode::RootNotFound);
            };
            let either = match identifier {
                PackageIdentifier::ContractPackageHash(hash) => {
                    get_contract_package(effect_builder, state_root_hash, hash).await
                }
                PackageIdentifier::PackageAddr(addr) => {
                    get_package(effect_builder, state_root_hash, addr)
                        .await
                        .map(|opt| opt.map(Either::Right))
                }
            };
            match either {
                Ok(Some(Either::Left(contract_package))) => {
                    BinaryResponse::from_value(contract_package)
                }
                Ok(Some(Either::Right(package))) => BinaryResponse::from_value(package),
                Ok(None) => BinaryResponse::new_empty(),
                Err(err) => BinaryResponse::new_error(err),
            }
        }
        InformationRequest::Entity {
            state_identifier,
            identifier,
            include_bytecode,
        } => {
            let Some(state_root_hash) =
                resolve_state_root_hash(effect_builder, state_identifier).await
            else {
                return BinaryResponse::new_error(ErrorCode::RootNotFound);
            };
            match identifier {
                EntityIdentifier::ContractHash(hash) => {
                    match get_contract(effect_builder, state_root_hash, hash, include_bytecode)
                        .await
                    {
                        Ok(Some(Either::Left(contract))) => BinaryResponse::from_value(contract),
                        Ok(Some(Either::Right(entity))) => BinaryResponse::from_value(entity),
                        Ok(None) => BinaryResponse::new_empty(),
                        Err(err) => BinaryResponse::new_error(err),
                    }
                }
                EntityIdentifier::AccountHash(hash) => {
                    match get_account(effect_builder, state_root_hash, hash, include_bytecode).await
                    {
                        Ok(Some(Either::Left(account))) => BinaryResponse::from_value(account),
                        Ok(Some(Either::Right(entity))) => BinaryResponse::from_value(entity),
                        Ok(None) => BinaryResponse::new_empty(),
                        Err(err) => BinaryResponse::new_error(err),
                    }
                }
                EntityIdentifier::PublicKey(pub_key) => {
                    let hash = pub_key.to_account_hash();
                    match get_account(effect_builder, state_root_hash, hash, include_bytecode).await
                    {
                        Ok(Some(Either::Left(account))) => BinaryResponse::from_value(account),
                        Ok(Some(Either::Right(entity))) => BinaryResponse::from_value(entity),
                        Ok(None) => BinaryResponse::new_empty(),
                        Err(err) => BinaryResponse::new_error(err),
                    }
                }
                EntityIdentifier::EntityAddr(addr) => {
                    match get_entity(effect_builder, state_root_hash, addr, include_bytecode).await
                    {
                        Ok(Some(entity)) => BinaryResponse::from_value(entity),
                        Ok(None) => BinaryResponse::new_empty(),
                        Err(err) => BinaryResponse::new_error(err),
                    }
                }
            }
        }
    }
}

async fn try_accept_transaction<REv>(
    effect_builder: EffectBuilder<REv>,
    transaction: Transaction,
    is_speculative: bool,
) -> BinaryResponse
where
    REv: From<AcceptTransactionRequest>,
{
    effect_builder
        .try_accept_transaction(transaction, is_speculative)
        .await
        .map_or_else(
            |err| BinaryResponse::new_error(err.into()),
            |()| BinaryResponse::new_empty(),
        )
}

async fn try_speculative_execution<REv>(
    effect_builder: EffectBuilder<REv>,
    transaction: Transaction,
) -> BinaryResponse
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    let tip = match effect_builder
        .get_highest_complete_block_header_from_storage()
        .await
    {
        Some(tip) => tip,
        None => return BinaryResponse::new_error(ErrorCode::NoCompleteBlocks),
    };

    let result = effect_builder
        .speculatively_execute(Box::new(tip), Box::new(transaction))
        .await;

    match result {
        SpeculativeExecutionResult::InvalidTransaction(error) => {
            debug!(%error, "invalid transaction submitted for speculative execution");
            BinaryResponse::new_error(error.into())
        }
        SpeculativeExecutionResult::WasmV1(spec_exec_result) => {
            BinaryResponse::from_value(spec_exec_result)
        }
        SpeculativeExecutionResult::ReceivedV1Transaction => {
            BinaryResponse::new_error(ErrorCode::ReceivedV1Transaction)
        }
    }
}

async fn handle_client_loop<REv>(
    stream: TcpStream,
    effect_builder: EffectBuilder<REv>,
    config: Arc<Config>,
    rate_limiter: Arc<Mutex<RateLimiter>>,
    monitor: ConnectionTerminator,
    life_extensions_config: BinaryRequestTerminationDelayValues,
) -> Result<(), Error>
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<AcceptTransactionRequest>
        + From<NetworkInfoRequest>
        + From<ReactorInfoRequest>
        + From<ConsensusRequest>
        + From<BlockSynchronizerRequest>
        + From<UpgradeWatcherRequest>
        + From<ChainspecRawBytesRequest>
        + Send,
{
    let codec = BinaryMessageCodec::new(config.max_message_size_bytes);
    let mut framed = Framed::new(stream, codec);
    monitor
        .terminate_at(Timestamp::now() + config.initial_connection_lifetime)
        .await;
    let cancellation_token = monitor.get_cancellation_token();
    loop {
        select! {
            maybe_bytes = framed.next() => {
                let Some(result) = maybe_bytes else {
                    debug!("remote party closed the connection");
                    return Ok(());
                };
                let limiter_response = rate_limiter.lock().await.throttle();
                let binary_message = result?;
                let payload = binary_message.payload();
                if payload.is_empty() {
                    // This should be unreachable, we reject 0-length messages earlier
                    warn!("Empty payload detected late.");
                    return Err(Error::NoPayload);
                }
                let mut bytes_buf = bytes::BytesMut::with_capacity(payload.len() + 4);
                let response =
                    handle_payload(effect_builder, payload, limiter_response, &monitor, &life_extensions_config).await;
                codec.clone().encode(binary_message, &mut bytes_buf)?;
                framed
                    .send(BinaryMessage::new(
                        BinaryResponseAndRequest::new(response, Bytes::from(bytes_buf.freeze().to_vec())).to_bytes()?,
                    ))
                    .await?
            }
            _ = cancellation_token.cancelled() => {
                debug!("Binary port connection stale - closing.");
                return Ok(());
            }
        }
    }
}

fn extract_header(payload: &[u8]) -> Result<(CommandHeader, &[u8]), ErrorCode> {
    const BINARY_VERSION_LENGTH_BYTES: usize = size_of::<u16>();

    if payload.len() < BINARY_VERSION_LENGTH_BYTES {
        return Err(ErrorCode::TooLittleBytesForRequestHeaderVersion);
    }

    let binary_protocol_version = match u16::from_bytes(payload) {
        Ok((binary_protocol_version, _)) => binary_protocol_version,
        Err(_) => return Err(ErrorCode::MalformedCommandHeaderVersion),
    };

    if binary_protocol_version != CommandHeader::HEADER_VERSION {
        return Err(ErrorCode::CommandHeaderVersionMismatch);
    }

    match CommandHeader::from_bytes(payload) {
        Ok((header, remainder)) => Ok((header, remainder)),
        Err(error) => {
            debug!(%error, "failed to parse binary request header");
            Err(ErrorCode::MalformedCommandHeader)
        }
    }
}

async fn handle_payload<REv>(
    effect_builder: EffectBuilder<REv>,
    payload: &[u8],
    limiter_response: LimiterResponse,
    connection_terminator: &ConnectionTerminator,
    life_extensions_config: &BinaryRequestTerminationDelayValues,
) -> BinaryResponse
where
    REv: From<Event>,
{
    let (header, remainder) = match extract_header(payload) {
        Ok(header) => header,
        Err(error_code) => return BinaryResponse::new_error(error_code),
    };

    if let LimiterResponse::Throttled = limiter_response {
        return BinaryResponse::new_error(ErrorCode::RequestThrottled);
    }

    // we might receive a request added in a minor version if we're behind
    let Ok(tag) = CommandTag::try_from(header.type_tag()) else {
        return BinaryResponse::new_error(ErrorCode::UnsupportedRequest);
    };

    let request = match Command::try_from((tag, remainder)) {
        Ok(request) => request,
        Err(error) => {
            debug!(%error, "failed to parse binary request body");
            return BinaryResponse::new_error(ErrorCode::MalformedCommand);
        }
    };
    connection_terminator
        .delay_termination(life_extensions_config.get_life_termination_delay(&request))
        .await;

    effect_builder
        .make_request(
            |responder| Event::HandleRequest { request, responder },
            QueueKind::Regular,
        )
        .await
}

async fn handle_client<REv>(
    addr: SocketAddr,
    stream: TcpStream,
    effect_builder: EffectBuilder<REv>,
    config: Arc<Config>,
    _permit: OwnedSemaphorePermit,
    rate_limiter: Arc<Mutex<RateLimiter>>,
) where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<AcceptTransactionRequest>
        + From<NetworkInfoRequest>
        + From<ReactorInfoRequest>
        + From<ConsensusRequest>
        + From<BlockSynchronizerRequest>
        + From<UpgradeWatcherRequest>
        + From<ChainspecRawBytesRequest>
        + Send,
{
    let keep_alive_monitor = ConnectionTerminator::new();
    let life_extensions_config = BinaryRequestTerminationDelayValues::from_config(&config);
    if let Err(err) = handle_client_loop(
        stream,
        effect_builder,
        config,
        rate_limiter,
        keep_alive_monitor,
        life_extensions_config,
    )
    .await
    {
        // Low severity is used to prevent malicious clients from causing log floods.
        trace!(%addr, err=display_error(&err), "binary port client handler error");
    }
}

async fn run_server<REv>(
    local_addr: Arc<OnceCell<SocketAddr>>,
    effect_builder: EffectBuilder<REv>,
    config: Arc<Config>,
    shutdown_trigger: Arc<Notify>,
) where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<AcceptTransactionRequest>
        + From<NetworkInfoRequest>
        + From<ReactorInfoRequest>
        + From<ConsensusRequest>
        + From<BlockSynchronizerRequest>
        + From<UpgradeWatcherRequest>
        + From<ChainspecRawBytesRequest>
        + Send,
{
    let listener = match TcpListener::bind(&config.address).await {
        Ok(listener) => listener,
        Err(err) => {
            error!(%err, "unable to bind binary port listener");
            return;
        }
    };

    let bind_address = match listener.local_addr() {
        Ok(bind_address) => bind_address,
        Err(err) => {
            error!(%err, "unable to get local addr of binary port");
            return;
        }
    };

    local_addr.set(bind_address).unwrap();

    loop {
        select! {
            _ = shutdown_trigger.notified() => {
                break;
            }
            result = listener.accept() => match result {
                Ok((stream, peer)) => {
                    effect_builder
                        .make_request(
                            |responder| Event::AcceptConnection {
                                stream,
                                peer,
                                responder,
                            },
                            QueueKind::Regular,
                        )
                        .await;
                }
                Err(io_err) => {
                    info!(%io_err, "problem accepting binary port connection");
                }
            }
        }
    }
}

#[cfg(test)]
impl crate::reactor::Finalize for BinaryPort {
    fn finalize(mut self) -> BoxFuture<'static, ()> {
        self.shutdown_trigger.notify_one();
        async move {
            if let Some(handle) = self.server_join_handle.take() {
                handle.await.ok();
            }
        }
        .boxed()
    }
}

async fn resolve_block_header<REv>(
    effect_builder: EffectBuilder<REv>,
    block_identifier: Option<BlockIdentifier>,
) -> Option<BlockHeader>
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    match block_identifier {
        Some(BlockIdentifier::Hash(block_hash)) => {
            effect_builder
                .get_block_header_from_storage(block_hash, true)
                .await
        }
        Some(BlockIdentifier::Height(block_height)) => {
            effect_builder
                .get_block_header_at_height_from_storage(block_height, true)
                .await
        }
        None => {
            effect_builder
                .get_highest_complete_block_header_from_storage()
                .await
        }
    }
}

async fn resolve_block_height<REv>(
    effect_builder: EffectBuilder<REv>,
    block_identifier: Option<BlockIdentifier>,
) -> Option<u64>
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    match block_identifier {
        Some(BlockIdentifier::Hash(block_hash)) => effect_builder
            .get_block_header_from_storage(block_hash, true)
            .await
            .map(|header| header.height()),
        Some(BlockIdentifier::Height(block_height)) => Some(block_height),
        None => effect_builder
            .get_highest_complete_block_from_storage()
            .await
            .map(|header| header.height()),
    }
}

async fn resolve_state_root_hash<REv>(
    effect_builder: EffectBuilder<REv>,
    state_identifier: Option<GlobalStateIdentifier>,
) -> Option<Digest>
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    match state_identifier {
        Some(GlobalStateIdentifier::BlockHash(block_hash)) => effect_builder
            .get_block_header_from_storage(block_hash, true)
            .await
            .map(|header| *header.state_root_hash()),
        Some(GlobalStateIdentifier::BlockHeight(block_height)) => effect_builder
            .get_block_header_at_height_from_storage(block_height, true)
            .await
            .map(|header| *header.state_root_hash()),
        Some(GlobalStateIdentifier::StateRootHash(state_root_hash)) => Some(state_root_hash),
        None => effect_builder
            .get_highest_complete_block_header_from_storage()
            .await
            .map(|header| *header.state_root_hash()),
    }
}

async fn resolve_era_switch_block_header<REv>(
    effect_builder: EffectBuilder<REv>,
    era_identifier: Option<EraIdentifier>,
) -> Option<BlockHeader>
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    match era_identifier {
        Some(EraIdentifier::Era(era_id)) => {
            effect_builder
                .get_switch_block_header_by_era_id_from_storage(era_id)
                .await
        }
        Some(EraIdentifier::Block(block_identifier)) => {
            let header = resolve_block_header(effect_builder, Some(block_identifier)).await?;
            if header.is_switch_block() {
                Some(header)
            } else {
                effect_builder
                    .get_switch_block_header_by_era_id_from_storage(header.era_id())
                    .await
            }
        }
        None => {
            effect_builder
                .get_latest_switch_block_header_from_storage()
                .await
        }
    }
}

impl<REv> Component<REv> for BinaryPort
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<AcceptTransactionRequest>
        + From<NetworkInfoRequest>
        + From<ReactorInfoRequest>
        + From<ConsensusRequest>
        + From<BlockSynchronizerRequest>
        + From<UpgradeWatcherRequest>
        + From<ChainspecRawBytesRequest>
        + Send,
{
    type Event = Event;

    fn name(&self) -> &str {
        COMPONENT_NAME
    }

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.state {
            ComponentState::Uninitialized => {
                warn!(
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when component is uninitialized"
                );
                Effects::new()
            }
            ComponentState::Initializing => match event {
                Event::Initialize => {
                    let rate_limiter_res =
                        RateLimiter::new(self.config.qps_limit, TimeDiff::from_seconds(1));
                    match rate_limiter_res {
                        Ok(rate_limiter) => {
                            match self.rate_limiter.set(Arc::new(Mutex::new(rate_limiter))) {
                                Ok(_) => {}
                                Err(_) => {
                                    error!("failed to initialize binary port, rate limiter already initialized");
                                    <Self as InitializedComponent<REv>>::set_state(
                                        self,
                                        ComponentState::Fatal("failed to initialize binary port, rate limiter already initialized".to_string()),
                                    );
                                    return Effects::new();
                                }
                            };
                        }
                        Err(error) => {
                            error!(%error, "failed to initialize binary port");
                            <Self as InitializedComponent<REv>>::set_state(
                                self,
                                ComponentState::Fatal(error.to_string()),
                            );
                            return Effects::new();
                        }
                    };
                    let (effects, state) = self.bind(self.config.enable_server, effect_builder);
                    <Self as InitializedComponent<MainEvent>>::set_state(self, state);
                    effects
                }
                _ => {
                    warn!(
                        ?event,
                        name = <Self as Component<MainEvent>>::name(self),
                        "binary port is initializing, ignoring event"
                    );
                    Effects::new()
                }
            },
            ComponentState::Initialized => match event {
                Event::Initialize => {
                    error!(
                        ?event,
                        name = <Self as Component<MainEvent>>::name(self),
                        "component already initialized"
                    );
                    Effects::new()
                }
                Event::AcceptConnection {
                    stream,
                    peer,
                    responder,
                } => {
                    if let Ok(permit) = Arc::clone(&self.connection_limit).try_acquire_owned() {
                        self.metrics.binary_port_connections_count.inc();
                        let config = Arc::clone(&self.config);
                        let rate_limiter = Arc::clone(
                            self.rate_limiter
                                .get()
                                .expect("This should have been set during initialization"),
                        );
                        tokio::spawn(handle_client(
                            peer,
                            stream,
                            effect_builder,
                            config,
                            permit,
                            rate_limiter,
                        ));
                    } else {
                        warn!(
                            "connection limit reached, dropping connection from {}",
                            peer
                        );
                    }
                    responder.respond(()).ignore()
                }
                Event::HandleRequest { request, responder } => {
                    let config = Arc::clone(&self.config);
                    let metrics = Arc::clone(&self.metrics);
                    let protocol_version = self.chainspec.protocol_version();
                    async move {
                        let response = handle_request(
                            request,
                            effect_builder,
                            &config,
                            &metrics,
                            protocol_version,
                        )
                        .await;
                        responder.respond(response).await;
                    }
                    .ignore()
                }
            },
            ComponentState::Fatal(msg) => {
                error!(
                    msg,
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
        }
    }
}

impl<REv> InitializedComponent<REv> for BinaryPort
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<AcceptTransactionRequest>
        + From<NetworkInfoRequest>
        + From<ReactorInfoRequest>
        + From<ConsensusRequest>
        + From<BlockSynchronizerRequest>
        + From<UpgradeWatcherRequest>
        + From<ChainspecRawBytesRequest>
        + Send,
{
    fn state(&self) -> &ComponentState {
        &self.state
    }

    fn set_state(&mut self, new_state: ComponentState) {
        info!(
            ?new_state,
            name = <Self as Component<MainEvent>>::name(self),
            "component state changed"
        );

        self.state = new_state;
    }
}

impl<REv> PortBoundComponent<REv> for BinaryPort
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<AcceptTransactionRequest>
        + From<NetworkInfoRequest>
        + From<ReactorInfoRequest>
        + From<ConsensusRequest>
        + From<BlockSynchronizerRequest>
        + From<UpgradeWatcherRequest>
        + From<ChainspecRawBytesRequest>
        + Send,
{
    type Error = ListeningError;
    type ComponentEvent = Event;

    fn listen(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Self::ComponentEvent>, Self::Error> {
        let local_addr = Arc::clone(&self.local_addr);
        let server_join_handle = tokio::spawn(run_server(
            local_addr,
            effect_builder,
            Arc::clone(&self.config),
            Arc::clone(&self.shutdown_trigger),
        ));
        self.server_join_handle
            .set(server_join_handle)
            .expect("server join handle should not be set elsewhere");

        Ok(Effects::new())
    }
}
