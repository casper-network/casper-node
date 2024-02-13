//! The Binary Port
mod config;
mod error;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use std::{convert::TryFrom, net::SocketAddr, sync::Arc};

use bytes::Bytes;
use casper_execution_engine::engine_state::{
    get_all_values::GetAllValuesRequest, QueryRequest, QueryResult,
};
use casper_types::{
    binary_port::{
        self, BinaryRequest, BinaryRequestHeader, BinaryRequestTag, BinaryResponse,
        BinaryResponseAndRequest, DbRawBytesSpec, GetAllValuesResult, GetRequest,
        GlobalStateQueryResult, GlobalStateRequest, InformationRequest, InformationRequestTag,
        NodeStatus, RecordId, TransactionWithExecutionInfo,
    },
    bytesrepr::{self, FromBytes, ToBytes},
    BlockHeader, BlockIdentifier, Digest, GlobalStateIdentifier, Peers, ProtocolVersion,
    SignedBlock, TimeDiff, Timestamp, Transaction,
};
use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use juliet::{
    io::IoCoreBuilder,
    protocol::ProtocolBuilder,
    rpc::{JulietRpcServer, RpcBuilder},
    ChannelConfiguration, ChannelId,
};
use once_cell::sync::OnceCell;
use prometheus::Registry;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    join,
    net::{TcpListener, TcpStream},
    sync::{Notify, OwnedSemaphorePermit, Semaphore},
};
use tracing::{debug, error, info, warn};

use crate::{
    contract_runtime::SpeculativeExecutionState,
    effect::{
        requests::{
            AcceptTransactionRequest, BlockSynchronizerRequest, ChainspecRawBytesRequest,
            ConsensusRequest, ContractRuntimeRequest, NetworkInfoRequest, ReactorInfoRequest,
            StorageRequest, UpgradeWatcherRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    reactor::{main_reactor::MainEvent, Finalize, QueueKind},
    types::NodeRng,
    utils::ListeningError,
};

use self::{error::Error, metrics::Metrics};

use super::{Component, ComponentState, InitializedComponent, PortBoundComponent};
pub(crate) use config::Config;
pub(crate) use event::Event;

const COMPONENT_NAME: &str = "binary_port";

#[derive(Debug, DataSize)]
pub(crate) struct BinaryPort {
    state: ComponentState,
    config: Arc<Config>,
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
}

impl BinaryPort {
    pub(crate) fn new(config: Config, registry: &Registry) -> Result<Self, prometheus::Error> {
        Ok(Self {
            state: ComponentState::Uninitialized,
            connection_limit: Arc::new(Semaphore::new(config.max_connections)),
            config: Arc::new(config),
            metrics: Arc::new(Metrics::new(registry)?),
            local_addr: Arc::new(OnceCell::new()),
            shutdown_trigger: Arc::new(Notify::new()),
            server_join_handle: OnceCell::new(),
        })
    }

    /// Returns the binding address.
    ///
    /// Only used in testing.
    #[cfg(test)]
    pub(crate) fn bind_address(&self) -> Option<SocketAddr> {
        self.local_addr.get().cloned()
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
                        tokio::spawn(handle_client(peer, stream, effect_builder, config, permit));
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
                    async move {
                        let response =
                            handle_request(request, effect_builder, &config, &metrics).await;
                        responder.respond(response).await
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

    fn name(&self) -> &str {
        COMPONENT_NAME
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

async fn handle_request<REv>(
    req: BinaryRequest,
    effect_builder: EffectBuilder<REv>,
    config: &Config,
    metrics: &Metrics,
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
    let version = effect_builder.get_protocol_version().await;
    match req {
        BinaryRequest::TryAcceptTransaction { transaction } => {
            metrics.binary_port_try_accept_transaction_count.inc();
            try_accept_transaction(effect_builder, transaction, None, version).await
        }
        BinaryRequest::TrySpeculativeExec {
            transaction,
            state_root_hash,
            block_time,
            protocol_version,
            speculative_exec_at_block,
        } => {
            metrics.binary_port_try_speculative_exec_count.inc();
            let response = try_accept_transaction(
                effect_builder,
                transaction.clone(),
                Some(speculative_exec_at_block),
                version,
            )
            .await;
            if !response.is_success() {
                return response;
            }
            try_speculative_execution(
                effect_builder,
                state_root_hash,
                block_time,
                protocol_version,
                transaction,
            )
            .await
        }
        BinaryRequest::Get(get_req) => {
            handle_get_request(get_req, effect_builder, config, metrics, version).await
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
            let Ok(block_hash) = bytesrepr::deserialize_from_slice(&key) else {
                return BinaryResponse::new_error(binary_port::ErrorCode::BadRequest, protocol_version);
            };
            let Some(transfers) = effect_builder
                .get_block_transfers_from_storage(block_hash)
                .await else {
                return BinaryResponse::new_empty(protocol_version);
            };
            let Ok(serialized) = bincode::serialize(&transfers) else {
                return BinaryResponse::new_error(binary_port::ErrorCode::InternalError, protocol_version);
            };
            let bytes = DbRawBytesSpec::new_current(&serialized);
            BinaryResponse::from_db_raw_bytes(RecordId::Transfer, Some(bytes), protocol_version)
        }
        GetRequest::Record {
            record_type_tag,
            key,
        } => {
            metrics.binary_port_get_record_count.inc();
            match RecordId::try_from(record_type_tag) {
                Ok(record_id) => {
                    let maybe_raw_bytes = effect_builder.get_raw_data(record_id, key).await;
                    BinaryResponse::from_db_raw_bytes(record_id, maybe_raw_bytes, protocol_version)
                }
                Err(_) => BinaryResponse::new_error(
                    binary_port::ErrorCode::UnsupportedRequest,
                    protocol_version,
                ),
            }
        }
        GetRequest::Information { info_type_tag, key } => {
            metrics.binary_port_get_info_count.inc();
            let Ok(tag) = InformationRequestTag::try_from(info_type_tag) else {
                return BinaryResponse::new_error(binary_port::ErrorCode::UnsupportedRequest, protocol_version);
            };
            let Ok(req) = InformationRequest::try_from((tag, &key[..])) else {
                return BinaryResponse::new_error(binary_port::ErrorCode::BadRequest, protocol_version);
            };
            handle_info_request(req, effect_builder, protocol_version).await
        }
        GetRequest::State(req) => {
            metrics.binary_port_get_state_count.inc();
            handle_state_request(effect_builder, req, protocol_version, config).await
        }
    }
}

async fn handle_get_all_items<REv>(
    state_identifier: Option<GlobalStateIdentifier>,
    key_tag: casper_types::KeyTag,
    effect_builder: EffectBuilder<REv>,
    protocol_version: ProtocolVersion,
) -> BinaryResponse
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    let Some(state_root_hash) = resolve_state_root_hash(effect_builder, state_identifier).await else {
        return BinaryResponse::new_empty(protocol_version)
    };
    let get_all_values_request = GetAllValuesRequest::new(state_root_hash, key_tag);
    match effect_builder.get_all_values(get_all_values_request).await {
        Ok(GetAllValuesResult::Success { values }) => {
            BinaryResponse::from_value(values, protocol_version)
        }
        Ok(GetAllValuesResult::RootNotFound) => {
            let error_code = binary_port::ErrorCode::RootNotFound;
            BinaryResponse::new_error(error_code, protocol_version)
        }
        Err(_err) => {
            BinaryResponse::new_error(binary_port::ErrorCode::InternalError, protocol_version)
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
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    match request {
        GlobalStateRequest::Item {
            state_identifier,
            base_key,
            path,
        } => {
            handle_get_item_request(
                effect_builder,
                state_identifier,
                base_key,
                path,
                protocol_version,
            )
            .await
        }
        GlobalStateRequest::AllItems {
            state_identifier,
            key_tag,
        } => {
            if !config.allow_request_get_all_values {
                BinaryResponse::new_error(
                    binary_port::ErrorCode::FunctionDisabled,
                    protocol_version,
                )
            } else {
                handle_get_all_items(state_identifier, key_tag, effect_builder, protocol_version)
                    .await
            }
        }
        GlobalStateRequest::Trie { trie_key } => {
            let response = if !config.allow_request_get_trie {
                BinaryResponse::new_error(
                    binary_port::ErrorCode::FunctionDisabled,
                    protocol_version,
                )
            } else {
                match effect_builder.get_trie_full(trie_key).await {
                    Ok(result) => BinaryResponse::from_value(result, protocol_version),
                    Err(_err) => BinaryResponse::new_error(
                        binary_port::ErrorCode::InternalError,
                        protocol_version,
                    ),
                }
            };
            response
        }
    }
}

async fn handle_get_item_request<REv>(
    effect_builder: EffectBuilder<REv>,
    state_identifier: Option<GlobalStateIdentifier>,
    base_key: casper_types::Key,
    path: Vec<String>,
    protocol_version: ProtocolVersion,
) -> BinaryResponse
where
    REv: From<Event> + From<ContractRuntimeRequest> + From<StorageRequest>,
{
    let Some(state_root_hash) = resolve_state_root_hash(effect_builder, state_identifier).await else {
        return BinaryResponse::new_empty(protocol_version)
    };

    match effect_builder
        .query_global_state(QueryRequest::new(state_root_hash, base_key, path))
        .await
    {
        Ok(QueryResult::Success { value, proofs }) => match proofs.to_bytes() {
            Ok(proofs) => BinaryResponse::from_value(
                GlobalStateQueryResult::new(*value, base16::encode_lower(&proofs)),
                protocol_version,
            ),
            Err(_) => {
                let error_code = binary_port::ErrorCode::InternalError;
                BinaryResponse::new_error(error_code, protocol_version)
            }
        },
        Ok(QueryResult::RootNotFound) => {
            let error_code = binary_port::ErrorCode::RootNotFound;
            BinaryResponse::new_error(error_code, protocol_version)
        }
        Ok(_) => {
            let error_code = binary_port::ErrorCode::NotFound;
            BinaryResponse::new_error(error_code, protocol_version)
        }
        Err(_) => {
            let error_code = binary_port::ErrorCode::QueryFailedToExecute;
            BinaryResponse::new_error(error_code, protocol_version)
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
            let Some(height) = resolve_block_height(effect_builder, identifier).await else {
                return BinaryResponse::new_empty(protocol_version);
            };
            let maybe_header = effect_builder
                .get_block_header_at_height_from_storage(height, true)
                .await;
            BinaryResponse::from_option(maybe_header, protocol_version)
        }
        InformationRequest::SignedBlock(identifier) => {
            let Some(height) = resolve_block_height(effect_builder, identifier).await else {
                return BinaryResponse::new_empty(protocol_version);
            };
            let Some(block) = effect_builder
                .get_block_at_height_with_metadata_from_storage(height, true)
                .await else {
                return BinaryResponse::new_empty(protocol_version);
            };
            BinaryResponse::from_value(
                SignedBlock::new(block.block, block.block_signatures),
                protocol_version,
            )
        }
        InformationRequest::Transaction(hash) => {
            let Some(transaction) = effect_builder.get_transaction_by_hash_from_storage(hash).await else {
                return BinaryResponse::new_empty(protocol_version);
            };
            let execution_info = effect_builder
                .get_transaction_execution_info_from_storage(hash)
                .await;
            BinaryResponse::from_value(
                TransactionWithExecutionInfo::new(transaction, execution_info),
                protocol_version,
            )
        }
        InformationRequest::Peers => BinaryResponse::from_value(
            Peers::from(effect_builder.network_peers().await),
            protocol_version,
        ),
        InformationRequest::Uptime => {
            BinaryResponse::from_value(effect_builder.get_uptime().await, protocol_version)
        }
        InformationRequest::LastProgress => {
            BinaryResponse::from_value(effect_builder.get_last_progress().await, protocol_version)
        }
        InformationRequest::ReactorState => {
            BinaryResponse::from_value(effect_builder.get_reactor_state().await, protocol_version)
        }
        InformationRequest::NetworkName => {
            BinaryResponse::from_value(effect_builder.get_network_name().await, protocol_version)
        }
        InformationRequest::ConsensusValidatorChanges => BinaryResponse::from_value(
            effect_builder.get_consensus_validator_changes().await,
            protocol_version,
        ),
        InformationRequest::BlockSynchronizerStatus => BinaryResponse::from_value(
            effect_builder.get_block_synchronizer_status().await,
            protocol_version,
        ),
        InformationRequest::AvailableBlockRange => BinaryResponse::from_value(
            effect_builder
                .get_available_block_range_from_storage()
                .await,
            protocol_version,
        ),
        InformationRequest::NextUpgrade => {
            BinaryResponse::from_option(effect_builder.get_next_upgrade().await, protocol_version)
        }
        InformationRequest::ConsensusStatus => {
            BinaryResponse::from_option(effect_builder.consensus_status().await, protocol_version)
        }
        InformationRequest::ChainspecRawBytes => BinaryResponse::from_value(
            (*effect_builder.get_chainspec_raw_bytes().await).clone(),
            protocol_version,
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

            let Ok(uptime) = TimeDiff::try_from(node_uptime) else {
                return BinaryResponse::new_error(
                    binary_port::ErrorCode::InternalError,
                    protocol_version,
                )
            };

            let status = NodeStatus {
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
            };
            BinaryResponse::from_value(status, protocol_version)
        }
    }
}

async fn try_accept_transaction<REv>(
    effect_builder: EffectBuilder<REv>,
    transaction: Transaction,
    speculative_exec_at: Option<BlockHeader>,
    protocol_version: ProtocolVersion,
) -> BinaryResponse
where
    REv: From<AcceptTransactionRequest>,
{
    effect_builder
        .try_accept_transaction(transaction, speculative_exec_at.map(Box::new))
        .await
        .map_or_else(
            |err| BinaryResponse::new_error(err.into(), protocol_version),
            |_| BinaryResponse::new_empty(protocol_version),
        )
}

async fn try_speculative_execution<REv>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    block_time: Timestamp,
    protocol_version: ProtocolVersion,
    transaction: Transaction,
) -> BinaryResponse
where
    REv: From<Event> + From<ContractRuntimeRequest>,
{
    effect_builder
        .speculatively_execute(
            SpeculativeExecutionState {
                state_root_hash,
                block_time,
                protocol_version,
            },
            Box::new(transaction),
        )
        .await
        .map_or_else(
            |err| BinaryResponse::new_error(err.into(), protocol_version),
            |val| BinaryResponse::from_value(val, protocol_version),
        )
}

async fn client_loop<REv, const N: usize, R, W>(
    mut server: JulietRpcServer<N, R, W>,
    effect_builder: EffectBuilder<REv>,
) -> Result<(), Error>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
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
    let version = effect_builder.get_protocol_version().await;
    loop {
        let Some(incoming_request) = server.next_request().await? else {
            debug!("remote party closed the connection");
            return Ok(());
        };

        let Some(payload) = incoming_request.payload() else {
            return Err(Error::NoPayload);
        };

        let resp = handle_payload(effect_builder, payload, version).await;
        let resp_and_payload = BinaryResponseAndRequest::new(resp, payload);
        incoming_request.respond(Some(Bytes::from(ToBytes::to_bytes(&resp_and_payload)?)))
    }
}

async fn handle_payload<REv>(
    effect_builder: EffectBuilder<REv>,
    payload: &[u8],
    protocol_version: ProtocolVersion,
) -> BinaryResponse
where
    REv: From<Event>,
{
    let Ok((header, remainder)) = BinaryRequestHeader::from_bytes(payload) else {
        return BinaryResponse::new_error(binary_port::ErrorCode::BadRequest, protocol_version);
    };

    if !header
        .protocol_version()
        .is_compatible_with(&protocol_version)
    {
        return BinaryResponse::new_error(
            binary_port::ErrorCode::UnsupportedProtocolVersion,
            protocol_version,
        );
    }

    // we might receive a request added in a minor version if we're behind
    let Ok(tag) = BinaryRequestTag::try_from(header.type_tag()) else {
        return BinaryResponse::new_error(binary_port::ErrorCode::UnsupportedRequest, protocol_version);
    };

    let Ok(request) = BinaryRequest::try_from((tag, remainder)) else {
        return BinaryResponse::new_error(binary_port::ErrorCode::BadRequest, protocol_version);
    };

    effect_builder
        .make_request(
            |responder| Event::HandleRequest { request, responder },
            QueueKind::Regular,
        )
        .await
}

async fn handle_client<REv>(
    addr: SocketAddr,
    mut client: TcpStream,
    effect_builder: EffectBuilder<REv>,
    config: Arc<Config>,
    _permit: OwnedSemaphorePermit,
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
    let (reader, writer) = client.split();
    // We are a server, we won't make any requests of our own, but we need to keep the client
    // around, since dropping the client will trigger a server shutdown.
    let (_client, server) = new_rpc_builder(&config).build(reader, writer);

    if let Err(err) = client_loop(server, effect_builder).await {
        // Low severity is used to prevent malicious clients from causing log floods.
        info!(%addr, %err, "binary port client handler error");
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
        tokio::select! {
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

impl Finalize for BinaryPort {
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

fn new_rpc_builder(config: &Config) -> RpcBuilder<1> {
    let protocol_builder = ProtocolBuilder::<1>::with_default_channel_config(
        ChannelConfiguration::default()
            .with_request_limit(config.client_request_limit)
            .with_max_request_payload_size(config.max_request_size_bytes)
            .with_max_response_payload_size(config.max_response_size_bytes),
    );
    let io_builder = IoCoreBuilder::new(protocol_builder)
        .buffer_size(ChannelId::new(0), config.client_request_buffer_size);
    RpcBuilder::new(io_builder)
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
