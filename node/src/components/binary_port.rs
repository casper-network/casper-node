//! The Binary Port
mod config;
mod error;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use std::{net::SocketAddr, sync::Arc};

use bytes::Bytes;
use casper_execution_engine::engine_state::{
    get_all_values::GetAllValuesRequest, Error as EngineStateError, QueryRequest,
};
use casper_types::{
    binary_port::{
        binary_request::BinaryRequest, get::GetRequest, global_state::GlobalStateQueryResult,
        non_persistent_data::NonPersistedDataRequest,
        speculative_execution::SpeculativeExecutionError,
    },
    bytesrepr::{FromBytes, ToBytes},
    BlockHashAndHeight, Transaction,
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
            AcceptTransactionRequest, BlockSynchronizerRequest, ConsensusRequest,
            ContractRuntimeRequest, NetworkInfoRequest, ReactorInfoRequest, StorageRequest,
            UpgradeWatcherRequest,
        },
        EffectBuilder, Effects,
    },
    reactor::{main_reactor::MainEvent, Finalize},
    types::{NodeRng, PeersMap},
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
        + Send,
{
    // TODO[RC]: clean this up, delegate to specialized functions
    match req {
        BinaryRequest::TryAcceptTransaction {
            transaction,
            speculative_exec_at_block,
        } => {
            let accept_transaction_result = effect_builder
                .try_accept_transaction(
                    Transaction::from(transaction),
                    speculative_exec_at_block
                        .map(|speculative_exec_at_block| Box::new(speculative_exec_at_block)),
                )
                .await
                .map_err(|err| Error::TransactionAcceptor(err).to_string()); // TODO[RC]: No string, but transaction acceptor error
            let bytes = ToBytes::to_bytes(&accept_transaction_result)
                .map_err(|err| Error::BytesRepr(err))?
                .into();
            Ok(Some(bytes))
        }
        BinaryRequest::SpeculativeExec {
            transaction,
            state_root_hash,
            block_time,
            protocol_version,
        } => {
            let execution_prestate = SpeculativeExecutionState {
                state_root_hash,
                block_time,
                protocol_version,
            };
            let speculative_execution_result = effect_builder
                .speculatively_execute(execution_prestate, Box::new(transaction))
                .await
                .map_err(|error| match error {
                    // TODO[RC]: Proper error conversion.
                    EngineStateError::RootNotFound(_) => SpeculativeExecutionError::NoSuchStateRoot,
                    EngineStateError::InvalidDeployItemVariant(error) => {
                        SpeculativeExecutionError::InvalidDeploy(error.to_string())
                    }
                    EngineStateError::WasmPreprocessing(error) => {
                        SpeculativeExecutionError::InvalidDeploy(error.to_string())
                    }

                    EngineStateError::InvalidProtocolVersion(_) => {
                        SpeculativeExecutionError::InvalidDeploy(format!(
                            "deploy used invalid protocol version {}",
                            error
                        ))
                    }
                    EngineStateError::Deploy => SpeculativeExecutionError::InvalidDeploy("".into()),
                    EngineStateError::Genesis(_)
                    | EngineStateError::WasmSerialization(_)
                    | EngineStateError::Exec(_)
                    | EngineStateError::Storage(_)
                    | EngineStateError::Authorization
                    | EngineStateError::InsufficientPayment
                    | EngineStateError::GasConversionOverflow
                    | EngineStateError::Finalization
                    | EngineStateError::Bytesrepr(_)
                    | EngineStateError::Mint(_)
                    | EngineStateError::InvalidKeyVariant
                    | EngineStateError::ProtocolUpgrade(_)
                    | EngineStateError::CommitError(_)
                    | EngineStateError::MissingSystemContractRegistry
                    | EngineStateError::MissingSystemContractHash(_)
                    | EngineStateError::RuntimeStackOverflow
                    | EngineStateError::FailedToGetKeys(_)
                    | EngineStateError::FailedToGetStoredWithdraws
                    | EngineStateError::FailedToGetWithdrawPurses
                    | EngineStateError::FailedToRetrieveUnbondingDelay
                    | EngineStateError::FailedToRetrieveEraId => {
                        SpeculativeExecutionError::InternalError(error.to_string())
                    }

                    _ => SpeculativeExecutionError::InternalError(format!(
                        "Unhandled engine state error: {}",
                        error
                    )),
                });

            let bytes = ToBytes::to_bytes(&speculative_execution_result)
                .map_err(|err| Error::BytesRepr(err))?
                .into();
            Ok(Some(bytes))
        }
        BinaryRequest::Get(req) => match req {
            GetRequest::Db { db, key } => Ok(effect_builder
                .get_raw_data(db, key)
                .await
                .map(|raw_data| Bytes::from(raw_data))),
            GetRequest::NonPersistedData(req) => match req {
                NonPersistedDataRequest::BlockHeight2Hash { height } => {
                    let block_hash = effect_builder.get_block_hash_for_height(height).await;
                    let payload = block_hash
                        .map(|data| data.to_bytes().map(Bytes::from))
                        .transpose()
                        .map_err(|err| Error::BytesRepr(err))?;
                    Ok(payload)
                }
                NonPersistedDataRequest::HighestCompleteBlock => {
                    let block_hash_and_height = effect_builder
                        .get_highest_complete_block_header_from_storage()
                        .await
                        .map(|block_header| {
                            BlockHashAndHeight::new(
                                block_header.block_hash(),
                                block_header.height(),
                            )
                        });
                    let payload = block_hash_and_height
                        .map(|data| data.to_bytes().map(Bytes::from))
                        .transpose()
                        .map_err(|err| Error::BytesRepr(err))?;
                    Ok(payload)
                }
                NonPersistedDataRequest::CompletedBlockContains { block_hash } => {
                    let val = effect_builder
                        .highest_completed_block_sequence_contains_hash(block_hash)
                        .await;
                    let payload = ToBytes::to_bytes(&val).map_err(|err| Error::BytesRepr(err))?;
                    Ok(Some(Bytes::from(payload)))
                }
                NonPersistedDataRequest::TransactionHash2BlockHashAndHeight {
                    transaction_hash,
                } => {
                    let block_hash_and_height = effect_builder
                        .get_block_hash_and_height_for_transaction(transaction_hash)
                        .await;
                    let payload = block_hash_and_height
                        .map(|data| data.to_bytes().map(Bytes::from))
                        .transpose()
                        .map_err(|err| Error::BytesRepr(err))?;
                    Ok(payload)
                }
                NonPersistedDataRequest::Peers => {
                    let peers = effect_builder.network_peers().await;
                    let peers_map = PeersMap::from(peers);
                    let payload =
                        ToBytes::to_bytes(&peers_map).map_err(|err| Error::BytesRepr(err))?;
                    Ok(Some(Bytes::from(payload)))
                }
                NonPersistedDataRequest::Uptime => {
                    let uptime = effect_builder.get_uptime().await.as_secs();
                    let payload =
                        ToBytes::to_bytes(&uptime).map_err(|err| Error::BytesRepr(err))?;
                    Ok(Some(Bytes::from(payload)))
                }
                NonPersistedDataRequest::LastProgress => {
                    let last_progress = effect_builder.get_last_progress().await;
                    let payload =
                        ToBytes::to_bytes(&last_progress).map_err(|err| Error::BytesRepr(err))?;
                    Ok(Some(Bytes::from(payload)))
                }
                NonPersistedDataRequest::ReactorState => {
                    let reactor_state = effect_builder.get_reactor_state().await;
                    let payload =
                        ToBytes::to_bytes(&reactor_state).map_err(|err| Error::BytesRepr(err))?;
                    Ok(Some(Bytes::from(payload)))
                }
                NonPersistedDataRequest::NetworkName => {
                    let network_name = effect_builder.get_network_name().await;
                    let payload =
                        ToBytes::to_bytes(&network_name).map_err(|err| Error::BytesRepr(err))?;
                    Ok(Some(Bytes::from(payload)))
                }
                NonPersistedDataRequest::ConsensusValidatorChanges => {
                    let consensus_validator_changes =
                        effect_builder.get_consensus_validator_changes().await;
                    let payload = ToBytes::to_bytes(&consensus_validator_changes)
                        .map_err(|err| Error::BytesRepr(err))?;
                    Ok(Some(Bytes::from(payload)))
                }
                NonPersistedDataRequest::BlockSynchronizerStatus => {
                    let block_synchronizer_status =
                        effect_builder.get_block_synchronizer_status().await;
                    let payload = ToBytes::to_bytes(&block_synchronizer_status)
                        .map_err(|err| Error::BytesRepr(err))?;
                    Ok(Some(Bytes::from(payload)))
                }
                NonPersistedDataRequest::AvailableBlockRange => {
                    let available_block_range = effect_builder
                        .get_available_block_range_from_storage()
                        .await;
                    let payload = ToBytes::to_bytes(&available_block_range)
                        .map_err(|err| Error::BytesRepr(err))?;
                    Ok(Some(Bytes::from(payload)))
                }
                NonPersistedDataRequest::NextUpgrade => {
                    let next_upgrade = effect_builder.get_next_upgrade().await;
                    let payload = next_upgrade
                        .map(|data| data.to_bytes().map(Bytes::from))
                        .transpose()
                        .map_err(|err| Error::BytesRepr(err))?;
                    Ok(payload)
                }
                NonPersistedDataRequest::ConsensusStatus => {
                    let consensus_status = effect_builder.consensus_status().await;
                    let payload = consensus_status
                        .map(|data| data.to_bytes().map(Bytes::from))
                        .transpose()
                        .map_err(|err| Error::BytesRepr(err))?;
                    Ok(payload)
                }
            },
            GetRequest::State {
                state_root_hash,
                base_key,
                path,
            } => {
                let query_result: GlobalStateQueryResult = effect_builder
                    .query_global_state(QueryRequest::new(state_root_hash, base_key, path))
                    .await
                    .map_err(|err| Error::EngineState(err))?
                    .into();

                let payload =
                    ToBytes::to_bytes(&query_result).map_err(|err| Error::BytesRepr(err))?;
                Ok(Some(payload.into()))
            }
            GetRequest::AllValues {
                state_root_hash,
                key_tag,
            } => {
                let get_all_values_request = GetAllValuesRequest::new(state_root_hash, key_tag);
                let get_all_values_result = effect_builder
                    .get_all_values(get_all_values_request)
                    .await
                    .map_err(|error| Error::EngineState(error))?;
                let bytes = ToBytes::to_bytes(&get_all_values_result)
                    .map_err(|err| Error::BytesRepr(err))?
                    .into();
                Ok(Some(bytes))
            }
            GetRequest::Trie { trie_key } => {
                let maybe_trie_bytes = effect_builder
                    .get_trie_full(trie_key)
                    .await
                    .map_err(|error| Error::EngineState(error))?;
                let payload = maybe_trie_bytes.map(|bytes| Bytes::from(bytes.take_inner()));
                Ok(payload)
            }
        },
    }
}

// TODO[RC]: Move to Self::
// TODO[RC]: Handle graceful shutdown
async fn handle_client<REv, const N: usize>(
    addr: SocketAddr,
    mut client: TcpStream,
    rpc_builder: Arc<RpcBuilder<N>>,
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
                        let response = handle_request(req, effect_builder).await;
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

    let listener = TcpListener::bind(config.address).await;
    match listener {
        Ok(listener) => loop {
            match listener.accept().await {
                Ok((client, addr)) => {
                    let rpc_builder_clone = Arc::clone(&rpc_builder);
                    tokio::spawn(handle_client(
                        addr,
                        client,
                        rpc_builder_clone,
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
