//! The Binary Port
mod config;
mod error;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use std::{net::SocketAddr, sync::Arc};

use bytes::{Buf, Bytes};
use casper_execution_engine::engine_state::{
    get_all_values::GetAllValuesRequest, Error as EngineStateError, QueryRequest,
};
use casper_types::{
    binary_port::{
        self,
        binary_request::BinaryRequest,
        binary_response::{self, BinaryResponse, BinaryResponseHeader},
        get::GetRequest,
        global_state::GlobalStateQueryResult,
        non_persistent_data::NonPersistedDataRequest,
        type_wrappers::NetworkName,
    },
    bytesrepr::{FromBytes, ToBytes},
    BlockHashAndHeight, BlockHeader, Peers, Transaction,
};
use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use juliet::{
    io::IoCoreBuilder, protocol::ProtocolBuilder, rpc::RpcBuilder, ChannelConfiguration, ChannelId,
};
use prometheus::Registry;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};

use crate::{
    contract_runtime::SpeculativeExecutionState,
    effect::{
        requests::{
            AcceptTransactionRequest, BlockSynchronizerRequest, ChainspecRawBytesRequest,
            ConsensusRequest, ContractRuntimeRequest, NetworkInfoRequest, ReactorInfoRequest,
            StorageRequest, UpgradeWatcherRequest,
        },
        EffectBuilder, Effects,
    },
    reactor::{main_reactor::MainEvent, Finalize},
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
    #[data_size(skip)]
    _metrics: Metrics,
    state: ComponentState,
    config: Config,
}

impl BinaryPort {
    pub(crate) fn new(config: Config, registry: &Registry) -> Result<Self, prometheus::Error> {
        Ok(Self {
            config,
            state: ComponentState::Uninitialized,
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
            },
            ComponentState::Initialized => {
                // Currently this component does not handle any events. Requests are handled
                // directly via the spawned `juliet` server.
                Effects::new()
            }
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
    config: &Config,
    effect_builder: EffectBuilder<REv>,
) -> Result<Option<Bytes>, Error>
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
    let temporarily_cloned_req = req.clone();

    // TODO[RC]: clean this up, delegate to specialized functions
    match req {
        BinaryRequest::TryAcceptTransaction { transaction } => {
            let response =
                try_accept_transaction(effect_builder, transaction, temporarily_cloned_req, None)
                    .await;

            let payload = ToBytes::to_bytes(&response).map_err(|err| Error::BytesRepr(err))?;
            Ok(Some(Bytes::from(payload)))
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
                temporarily_cloned_req.clone(),
                Some(speculative_exec_at_block),
            )
            .await;
            if response.is_error() {
                let payload = ToBytes::to_bytes(&response).map_err(|err| Error::BytesRepr(err))?;
                return Ok(Some(Bytes::from(payload)));
            }

            let execution_prestate = SpeculativeExecutionState {
                state_root_hash,
                block_time,
                protocol_version,
            };

            let speculative_execution_result = effect_builder
                .speculatively_execute(execution_prestate, Box::new(transaction))
                .await;

            let response = match speculative_execution_result {
                Ok(result) => BinaryResponse::from_value(temporarily_cloned_req, result),
                Err(err) => BinaryResponse::new_error(
                    match err {
                        EngineStateError::RootNotFound(_) => binary_port::Error::RootNotFound,
                        EngineStateError::InvalidDeployItemVariant(_) => {
                            binary_port::Error::InvalidDeployItemVariant
                        }
                        EngineStateError::WasmPreprocessing(_) => {
                            binary_port::Error::WasmPreprocessing
                        }
                        EngineStateError::InvalidProtocolVersion(_) => {
                            binary_port::Error::InvalidProtocolVersion
                        }
                        EngineStateError::Deploy => binary_port::Error::InvalidDeploy,
                        _ => binary_port::Error::InternalError,
                    },
                    temporarily_cloned_req,
                ),
            };

            let payload = ToBytes::to_bytes(&response).map_err(|err| Error::BytesRepr(err))?;
            Ok(Some(Bytes::from(payload)))
        }
        BinaryRequest::Get(get_req) => match get_req {
            GetRequest::Db { db, key } => {
                let maybe_raw_bytes = effect_builder.get_raw_data(db, key).await;
                let binary_response =
                    BinaryResponse::from_db_raw_bytes(&db, temporarily_cloned_req, maybe_raw_bytes);
                let payload =
                    ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?;
                Ok(Some(Bytes::from(payload)))
            }
            GetRequest::NonPersistedData(req) => match req {
                NonPersistedDataRequest::BlockHeight2Hash { height } => {
                    let binary_response = BinaryResponse::from_opt(
                        temporarily_cloned_req,
                        effect_builder.get_block_hash_for_height(height).await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::HighestCompleteBlock => {
                    let binary_response = BinaryResponse::from_opt(
                        temporarily_cloned_req,
                        effect_builder
                            .get_highest_complete_block_header_from_storage()
                            .await
                            .map(|block_header| {
                                BlockHashAndHeight::new(
                                    block_header.block_hash(),
                                    block_header.height(),
                                )
                            }),
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::CompletedBlocksContain { block_hash } => {
                    let binary_response = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        effect_builder
                            .highest_completed_block_sequence_contains_hash(block_hash)
                            .await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::TransactionHash2BlockHashAndHeight {
                    transaction_hash,
                } => {
                    let binary_response = BinaryResponse::from_opt(
                        temporarily_cloned_req,
                        effect_builder
                            .get_block_hash_and_height_for_transaction(transaction_hash)
                            .await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::Peers => {
                    let binary_response = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        Peers::from(effect_builder.network_peers().await),
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::Uptime => {
                    let uptime = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        effect_builder.get_uptime().await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&uptime).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::LastProgress => {
                    let binary_response = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        effect_builder.get_last_progress().await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::ReactorState => {
                    let binary_response = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        effect_builder.get_reactor_state().await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::NetworkName => {
                    let binary_response = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        effect_builder.get_network_name().await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::ConsensusValidatorChanges => {
                    let binary_response = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        effect_builder.get_consensus_validator_changes().await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::BlockSynchronizerStatus => {
                    let binary_response = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        effect_builder.get_block_synchronizer_status().await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::AvailableBlockRange => {
                    let binary_response = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        effect_builder
                            .get_available_block_range_from_storage()
                            .await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::NextUpgrade => {
                    let binary_response = BinaryResponse::from_opt(
                        temporarily_cloned_req,
                        effect_builder.get_next_upgrade().await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::ConsensusStatus => {
                    let binary_response = BinaryResponse::from_opt(
                        temporarily_cloned_req,
                        effect_builder.consensus_status().await,
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
                NonPersistedDataRequest::ChainspecRawBytes => {
                    let binary_response = BinaryResponse::from_value(
                        temporarily_cloned_req,
                        (*effect_builder.get_chainspec_raw_bytes().await).clone(),
                    );
                    Ok(Some(Bytes::from(
                        ToBytes::to_bytes(&binary_response).map_err(|err| Error::BytesRepr(err))?,
                    )))
                }
            },
            GetRequest::State {
                state_root_hash,
                base_key,
                path,
            } => {
                let response = match effect_builder
                    .query_global_state(QueryRequest::new(state_root_hash, base_key, path))
                    .await
                {
                    Ok(result) => {
                        let result: GlobalStateQueryResult = result.into();
                        BinaryResponse::from_value(temporarily_cloned_req, result)
                    }
                    Err(_err) => BinaryResponse::new_error(
                        binary_port::Error::GetStateFailed,
                        temporarily_cloned_req,
                    ),
                };

                let payload = ToBytes::to_bytes(&response).map_err(|err| Error::BytesRepr(err))?;
                Ok(Some(Bytes::from(payload)))
            }
            GetRequest::AllValues {
                state_root_hash,
                key_tag,
            } => {
                let response = if !config.allow_request_get_all_values {
                    BinaryResponse::new_error(
                        binary_port::Error::FunctionIsDisabled,
                        temporarily_cloned_req,
                    )
                } else {
                    let get_all_values_request = GetAllValuesRequest::new(state_root_hash, key_tag);
                    match effect_builder.get_all_values(get_all_values_request).await {
                        Ok(result) => BinaryResponse::from_value(temporarily_cloned_req, result),
                        Err(_err) => BinaryResponse::new_error(
                            binary_port::Error::GetAllValuesFailed,
                            temporarily_cloned_req,
                        ),
                    }
                };

                let payload = ToBytes::to_bytes(&response).map_err(|err| Error::BytesRepr(err))?;
                Ok(Some(Bytes::from(payload)))
            }
            GetRequest::Trie { trie_key } => {
                todo!()
                // if !config.allow_request_get_trie {
                //     return Err(Error::FunctionDisabled("GetRequest::Trie".to_string()));
                // }

                // let maybe_trie_bytes = effect_builder
                //     .get_trie_full(trie_key)
                //     .await
                //     .map_err(|error| Error::EngineState(error))?;
                // let payload = maybe_trie_bytes.map(|bytes| Bytes::from(bytes.take_inner()));
                // Ok(payload)
            }
        },
    }
}

async fn try_accept_transaction<REv>(
    effect_builder: EffectBuilder<REv>,
    transaction: Transaction,
    temporarily_cloned_req: BinaryRequest,
    speculative_exec_at: Option<BlockHeader>,
) -> BinaryResponse
where
    REv: From<AcceptTransactionRequest>,
{
    match effect_builder
        .try_accept_transaction(
            transaction,
            speculative_exec_at.map(|block_header| Box::new(block_header)),
        )
        .await
    {
        Ok(_) => BinaryResponse {
            header: BinaryResponseHeader::new(None),
            original_request: ToBytes::to_bytes(&temporarily_cloned_req).unwrap(),
            payload: vec![],
        },
        Err(_err) => {
            // TODO[RC]: Should we send more details to the sidecar?
            BinaryResponse::new_error(
                binary_port::Error::TransactionNotAccepted,
                temporarily_cloned_req,
            )
        }
    }
}

// TODO[RC]: Move to Self::
// TODO[RC]: Handle graceful shutdown
async fn handle_client<REv, const N: usize>(
    addr: SocketAddr,
    mut client: TcpStream,
    rpc_builder: Arc<RpcBuilder<N>>,
    config: Arc<Config>,
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
    let (client, mut server) = rpc_builder.build(reader, writer);

    loop {
        match server.next_request().await {
            Ok(Some(incoming_request)) => match incoming_request.payload() {
                Some(payload) => match BinaryRequest::from_bytes(payload.as_ref()) {
                    Ok((_, reminder)) if !reminder.is_empty() => {
                        info!("binary request leftover bytes detected, closing connection");
                        break;
                    }
                    Ok((req, _)) => {
                        let response = handle_request(req, &*config, effect_builder).await;
                        match response {
                            Ok(response) => incoming_request.respond(response),
                            Err(err) => {
                                info!(
                                    %err,
                                    "unable to serialize binary response, closing connection"
                                );
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        info!(
                            %err,
                            "unable to deserialize binary request, closing connection"
                        );
                        break;
                    }
                },
                None => {
                    info!("binary request without payload, closing connection");
                    break;
                }
            },
            Ok(None) => {
                debug!("remote party closed the connection");
                break;
            }
            Err(err) => {
                warn!(%addr, %err, "closing connection");
                break;
            }
        }
    }

    // We are a server, we won't make any requests of our own, but we need to keep the client
    // around, since dropping the client will trigger a server shutdown.
    drop(client);
}

// TODO[RC]: Move to Self::
async fn run_server<REv>(effect_builder: EffectBuilder<REv>, config: Config)
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
    let protocol_builder = ProtocolBuilder::<1>::with_default_channel_config(
        ChannelConfiguration::default()
            .with_request_limit(3)
            .with_max_request_payload_size(4 * 1024 * 1024)
            .with_max_response_payload_size(4 * 1024 * 1024),
    );
    let io_builder = IoCoreBuilder::new(protocol_builder).buffer_size(ChannelId::new(0), 16);
    let rpc_builder = Arc::new(RpcBuilder::new(io_builder));
    let config = Arc::new(config);

    let listener = TcpListener::bind(&config.address).await;

    // TODO[RC]: Temporary code for `from_value()` testing.
    // let br = BinaryRequest::Get(GetRequest::NonPersistedData(NonPersistedDataRequest::Peers));
    // let binary_response =
    //     BinaryResponse::from_value(br, PeersMap::from(effect_builder.network_peers().await));
    // error!("XXXXX - should be seen");
    // let br = BinaryRequest::Get(GetRequest::NonPersistedData(NonPersistedDataRequest::Peers));
    // let binary_response = BinaryResponse::from_value(
    //     br,
    //     Some(PeersMap::from(effect_builder.network_peers().await)),
    // );
    // error!("XXXXX - should NOT be seen");

    match listener {
        Ok(listener) => loop {
            match listener.accept().await {
                Ok((client, addr)) => {
                    let rpc_builder_clone = Arc::clone(&rpc_builder);
                    let config_clone = Arc::clone(&config);
                    tokio::spawn(handle_client(
                        addr,
                        client,
                        rpc_builder_clone,
                        config_clone,
                        effect_builder,
                    ));
                }
                Err(io_err) => {
                    println!("acceptance failure: {:?}", io_err);
                }
            }
        },
        Err(_) => (), // TODO[RC]: Handle this
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
        let _server_join_handle = tokio::spawn(run_server(effect_builder, self.config.clone()));
        Ok(Effects::new())
    }
}

impl Finalize for BinaryPort {
    fn finalize(self) -> BoxFuture<'static, ()> {
        // TODO: Shutdown juliet server here
        async move {}.boxed()
    }
}
