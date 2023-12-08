//! The Binary Port
mod config;
mod error;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use std::{convert::TryInto, net::SocketAddr, sync::Arc};

use bytes::Bytes;
use casper_execution_engine::engine_state::{
    get_all_values::GetAllValuesRequest, Error as EngineStateError, QueryRequest, QueryResult,
};
use casper_types::{
    binary_port::{
        self, binary_request::BinaryRequest, db_id::DbId, get::GetRequest,
        get_all_values::GetAllValuesResult, global_state::GlobalStateQueryResult,
        non_persistent_data::NonPersistedDataRequest, DbRawBytesSpec, NodeStatus,
    },
    bytesrepr::{self, FromBytes, ToBytes},
    BinaryResponse, BinaryResponseAndRequest, BlockHashAndHeight, BlockHeader, Peers, Transaction,
};
use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use juliet::{
    io::IoCoreBuilder,
    protocol::ProtocolBuilder,
    rpc::{JulietRpcServer, RpcBuilder},
    ChannelConfiguration, ChannelId,
};
use prometheus::Registry;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    join,
    net::{TcpListener, TcpStream},
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
        EffectBuilder, EffectExt, Effects, Responder,
    },
    reactor::{main_reactor::MainEvent, Finalize, QueueKind},
    types::NodeRng,
    utils::ListeningError,
};

use self::{error::Error, metrics::Metrics};

use super::{
    transaction_acceptor, Component, ComponentState, InitializedComponent, PortBoundComponent,
};
pub(crate) use config::Config;
pub(crate) use event::Event;

const COMPONENT_NAME: &str = "binary_port";

#[derive(Debug, DataSize)]
pub(crate) struct BinaryPort {
    #[data_size(skip)]
    _metrics: Metrics,
    state: ComponentState,
    config: Arc<Config>,
}

impl BinaryPort {
    pub(crate) fn new(config: Config, registry: &Registry) -> Result<Self, prometheus::Error> {
        Ok(Self {
            state: ComponentState::Uninitialized,
            config: Arc::new(config),
            _metrics: Metrics::new(registry)?,
        })
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
                Event::AcceptConnection { stream, peer } => {
                    tokio::spawn(handle_client(peer, stream, effect_builder));
                    Effects::new()
                }
                Event::HandleRequest { request, responder } => {
                    let config = Arc::clone(&self.config);
                    async move {
                        let response = handle_request(request, effect_builder, &config).await;
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
    // TODO[RC]: clean this up, delegate to specialized functions
    match req {
        // Related RPC errors:
        // - ErrorCode::InvalidDeploy -->
        BinaryRequest::TryAcceptTransaction { transaction } => {
            try_accept_transaction(effect_builder, transaction, None).await
        }
        BinaryRequest::TrySpeculativeExec {
            transaction,
            state_root_hash,
            block_time,
            protocol_version,
            speculative_exec_at_block,
        } => {
            let response = try_accept_transaction(
                effect_builder,
                transaction.clone(),
                Some(speculative_exec_at_block),
            )
            .await;
            if !response.is_success() {
                return response;
            }

            let execution_prestate = SpeculativeExecutionState {
                state_root_hash,
                block_time,
                protocol_version,
            };

            let speculative_execution_result = effect_builder
                .speculatively_execute(execution_prestate, Box::new(transaction))
                .await;

            match speculative_execution_result {
                Ok(result) => BinaryResponse::from_value(result),
                Err(err) => BinaryResponse::new_error(match err {
                    EngineStateError::RootNotFound(_) => binary_port::ErrorCode::RootNotFound,
                    EngineStateError::InvalidDeployItemVariant(_) => {
                        binary_port::ErrorCode::InvalidDeployItemVariant
                    }
                    EngineStateError::WasmPreprocessing(_) => {
                        binary_port::ErrorCode::WasmPreprocessing
                    }
                    EngineStateError::InvalidProtocolVersion(_) => {
                        binary_port::ErrorCode::InvalidProtocolVersion
                    }
                    EngineStateError::Deploy => binary_port::ErrorCode::InvalidDeploy,
                    _ => binary_port::ErrorCode::InternalError,
                }),
            }
        }
        BinaryRequest::Get(get_req) => match get_req {
            // this workaround is in place because get_block_transfers performs a lazy migration
            GetRequest::Db { db, key } if db == DbId::Transfer => {
                let Ok(block_hash) = bytesrepr::deserialize_from_slice(&key) else {
                    return BinaryResponse::new_error(binary_port::ErrorCode::BadRequest);
                };
                let Some(transfers) = effect_builder
                    .get_block_transfers_from_storage(block_hash)
                    .await else {
                    return BinaryResponse::from_db_raw_bytes(&db, None);
                };
                let serialized =
                    bincode::serialize(&transfers).expect("should serialize transfers to bytes");
                let bytes = DbRawBytesSpec::new_legacy(&serialized);
                BinaryResponse::from_db_raw_bytes(&db, Some(bytes))
            }
            GetRequest::Db { db, key } => {
                let maybe_raw_bytes = effect_builder.get_raw_data(db, key).await;
                BinaryResponse::from_db_raw_bytes(&db, maybe_raw_bytes)
            }
            GetRequest::NonPersistedData(req) => match req {
                NonPersistedDataRequest::BlockHeight2Hash { height } => {
                    BinaryResponse::from_opt(effect_builder.get_block_hash_for_height(height).await)
                }
                NonPersistedDataRequest::HighestCompleteBlock => BinaryResponse::from_opt(
                    effect_builder
                        .get_highest_complete_block_header_from_storage()
                        .await
                        .map(|block_header| {
                            BlockHashAndHeight::new(
                                block_header.block_hash(),
                                block_header.height(),
                            )
                        }),
                ),
                NonPersistedDataRequest::CompletedBlocksContain { block_identifier } => {
                    BinaryResponse::from_value(
                        effect_builder
                            .highest_completed_block_sequence_contains(block_identifier)
                            .await,
                    )
                }
                NonPersistedDataRequest::TransactionHash2BlockHashAndHeight {
                    transaction_hash,
                } => BinaryResponse::from_opt(
                    effect_builder
                        .get_block_hash_and_height_for_transaction(transaction_hash)
                        .await,
                ),
                NonPersistedDataRequest::Peers => {
                    BinaryResponse::from_value(Peers::from(effect_builder.network_peers().await))
                }
                NonPersistedDataRequest::Uptime => {
                    BinaryResponse::from_value(effect_builder.get_uptime().await)
                }
                NonPersistedDataRequest::LastProgress => {
                    BinaryResponse::from_value(effect_builder.get_last_progress().await)
                }
                NonPersistedDataRequest::ReactorState => {
                    BinaryResponse::from_value(effect_builder.get_reactor_state().await)
                }
                NonPersistedDataRequest::NetworkName => {
                    BinaryResponse::from_value(effect_builder.get_network_name().await)
                }
                NonPersistedDataRequest::ConsensusValidatorChanges => BinaryResponse::from_value(
                    effect_builder.get_consensus_validator_changes().await,
                ),
                NonPersistedDataRequest::BlockSynchronizerStatus => {
                    BinaryResponse::from_value(effect_builder.get_block_synchronizer_status().await)
                }
                NonPersistedDataRequest::AvailableBlockRange => BinaryResponse::from_value(
                    effect_builder
                        .get_available_block_range_from_storage()
                        .await,
                ),
                NonPersistedDataRequest::NextUpgrade => {
                    BinaryResponse::from_opt(effect_builder.get_next_upgrade().await)
                }
                NonPersistedDataRequest::ConsensusStatus => {
                    BinaryResponse::from_opt(effect_builder.consensus_status().await)
                }
                NonPersistedDataRequest::ChainspecRawBytes => BinaryResponse::from_value(
                    (*effect_builder.get_chainspec_raw_bytes().await).clone(),
                ),
                NonPersistedDataRequest::NodeStatus => {
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
                        consensus_status.map_or((None, None), |(pk, rl)| (Some(pk), rl));

                    let status = NodeStatus {
                        peers: Peers::from(peers),
                        build_version: crate::VERSION_STRING.clone(),
                        chainspec_name: network_name.into(),
                        starting_state_root_hash,
                        last_added_block_info: last_added_block.map(Into::into),
                        our_public_signing_key,
                        round_length,
                        next_upgrade,
                        uptime: node_uptime
                            .try_into()
                            .expect("uptime should fit into TimeDiff"),
                        reactor_state,
                        last_progress: last_progress.into(),
                        available_block_range,
                        block_sync,
                    };
                    BinaryResponse::from_value(status)
                }
            },
            GetRequest::State {
                state_root_hash,
                base_key,
                path,
            } => {
                match effect_builder
                    .query_global_state(QueryRequest::new(state_root_hash, base_key, path))
                    .await
                {
                    Ok(QueryResult::Success { value, proofs }) => match proofs.to_bytes() {
                        Ok(proofs) => BinaryResponse::from_value(GlobalStateQueryResult::new(
                            *value,
                            base16::encode_lower(&proofs),
                        )),
                        Err(_) => {
                            let error_code = binary_port::ErrorCode::InternalError;
                            BinaryResponse::new_error(error_code)
                        }
                    },
                    Ok(QueryResult::RootNotFound) => {
                        let error_code = binary_port::ErrorCode::RootNotFound;
                        BinaryResponse::new_error(error_code)
                    }
                    Ok(_) => {
                        let error_code = binary_port::ErrorCode::NotFound;
                        BinaryResponse::new_error(error_code)
                    }
                    Err(_) => {
                        let error_code = binary_port::ErrorCode::QueryFailedToExecute;
                        BinaryResponse::new_error(error_code)
                    }
                }
            }
            GetRequest::AllValues {
                state_root_hash,
                key_tag,
            } => {
                if !config.allow_request_get_all_values {
                    BinaryResponse::new_error(binary_port::ErrorCode::FunctionIsDisabled)
                } else {
                    let get_all_values_request = GetAllValuesRequest::new(state_root_hash, key_tag);
                    match effect_builder.get_all_values(get_all_values_request).await {
                        Ok(GetAllValuesResult::Success { values }) => {
                            BinaryResponse::from_value(values)
                        }
                        Ok(GetAllValuesResult::RootNotFound) => {
                            let error_code = binary_port::ErrorCode::RootNotFound;
                            BinaryResponse::new_error(error_code)
                        }
                        Err(_err) => {
                            BinaryResponse::new_error(binary_port::ErrorCode::InternalError)
                        }
                    }
                }
            }
            GetRequest::Trie { trie_key } => {
                let response = if !config.allow_request_get_trie {
                    BinaryResponse::new_error(binary_port::ErrorCode::FunctionIsDisabled)
                } else {
                    match effect_builder.get_trie_full(trie_key).await {
                        Ok(result) => BinaryResponse::from_value(result),
                        Err(_err) => {
                            BinaryResponse::new_error(binary_port::ErrorCode::InternalError)
                        }
                    }
                };
                response
            }
        },
    }
}

async fn try_accept_transaction<REv>(
    effect_builder: EffectBuilder<REv>,
    transaction: Transaction,
    speculative_exec_at: Option<BlockHeader>,
) -> BinaryResponse
where
    REv: From<AcceptTransactionRequest>,
{
    match effect_builder
        .try_accept_transaction(transaction, speculative_exec_at.map(Box::new))
        .await
    {
        Ok(_) => BinaryResponse::new_empty(),
        Err(err) => BinaryResponse::new_error(match err {
            transaction_acceptor::Error::EmptyBlockchain
            | transaction_acceptor::Error::InvalidDeployConfiguration(_)
            | transaction_acceptor::Error::InvalidV1Configuration(_)
            | transaction_acceptor::Error::Parameters { .. }
            | transaction_acceptor::Error::Expired { .. }
            | transaction_acceptor::Error::ExpectedDeploy
            | transaction_acceptor::Error::ExpectedTransactionV1 => {
                binary_port::ErrorCode::InvalidDeploy
            }
        }),
    }
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
    loop {
        let Some(incoming_request) = server.next_request().await? else {
            debug!("remote party closed the connection");
            return Ok(());
        };

        let Some(payload) = incoming_request.payload() else {
            return Err(Error::NoPayload);
        };

        match BinaryRequest::from_bytes(payload.as_ref())? {
            (_, reminder) if !reminder.is_empty() => {
                return Err(bytesrepr::Error::LeftOverBytes.into());
            }
            (req, _) => {
                let response = effect_builder
                    .make_request(
                        |responder| Event::HandleRequest {
                            request: req,
                            responder,
                        },
                        QueueKind::Regular,
                    )
                    .await;

                let response = BinaryResponseAndRequest::new(response, payload.as_ref());
                incoming_request.respond(Some(Bytes::from(ToBytes::to_bytes(&response)?)))
            }
        }
    }
}

async fn handle_client<REv>(
    addr: SocketAddr,
    mut client: TcpStream,
    effect_builder: EffectBuilder<REv>,
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
    let (client, server) = new_rpc_builder().build(reader, writer);

    if let Err(err) = client_loop(server, effect_builder).await {
        // Low severity is used to prevent malicious clients from causing log floods.
        info!(%addr, %err, "binary port client handler error");
    }

    // We are a server, we won't make any requests of our own, but we need to keep the client
    // around, since dropping the client will trigger a server shutdown.
    drop(client);
}

// TODO[RC]: Move to Self::
async fn run_server<REv>(effect_builder: EffectBuilder<REv>, config: Arc<Config>)
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
    let listener = TcpListener::bind(&config.address).await;

    match listener {
        Ok(listener) => loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    effect_builder
                        .make_request(
                            |_: Responder<()>| Event::AcceptConnection { stream, peer },
                            QueueKind::Regular,
                        )
                        .await;
                }
                Err(io_err) => {
                    println!("acceptance failure: {:?}", io_err);
                }
            }
        },
        Err(_) => todo!(), // TODO[RC]: Handle this
    };
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
        let _server_join_handle =
            tokio::spawn(run_server(effect_builder, Arc::clone(&self.config)));
        Ok(Effects::new())
    }
}

impl Finalize for BinaryPort {
    fn finalize(self) -> BoxFuture<'static, ()> {
        // TODO: Shutdown juliet server here
        async move {}.boxed()
    }
}

fn new_rpc_builder() -> RpcBuilder<1> {
    let protocol_builder = ProtocolBuilder::<1>::with_default_channel_config(
        ChannelConfiguration::default()
            .with_request_limit(3)
            .with_max_request_payload_size(4 * 1024 * 1024)
            .with_max_response_payload_size(4 * 1024 * 1024),
    );
    let io_builder = IoCoreBuilder::new(protocol_builder).buffer_size(ChannelId::new(0), 16);
    RpcBuilder::new(io_builder)
}
