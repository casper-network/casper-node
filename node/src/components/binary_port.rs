//! The Binary Port
mod config;
mod error;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use std::{net::SocketAddr, sync::Arc};

use bytes::Bytes;
use casper_execution_engine::engine_state::{Error as EngineStateError, QueryRequest};
use casper_types::{
    binary_port::{
        get_request::GetRequest, BinaryRequest, GlobalStateQueryResult, NonPersistedDataRequest,
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
use tracing::{debug, info, warn};

use crate::{
    components::binary_port::error::SpeculativeExecutionError,
    contract_runtime::SpeculativeExecutionState,
    effect::{
        requests::{AcceptTransactionRequest, ContractRuntimeRequest, StorageRequest},
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
            // TODO[RC]: Handle this
            ComponentState::Initialized => todo!(),
            ComponentState::Fatal(_) => todo!(),
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
        + From<AcceptTransactionRequest>,
{
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
                .map_err(|error| {
                    match error {
                        EngineStateError::RootNotFound(_) => {
                            Error::SpeculativeExecution(SpeculativeExecutionError::NoSuchStateRoot)
                        }
                        EngineStateError::InvalidDeployItemVariant(error) => {
                            Error::SpeculativeExecution(SpeculativeExecutionError::InvalidDeploy(
                                error.to_string(),
                            ))
                        }
                        EngineStateError::WasmPreprocessing(error) => Error::SpeculativeExecution(
                            SpeculativeExecutionError::InvalidDeploy(error.to_string()),
                        ),
                        EngineStateError::InvalidProtocolVersion(_) => {
                            Error::SpeculativeExecution(SpeculativeExecutionError::InvalidDeploy(
                                format!("deploy used invalid protocol version {}", error),
                            ))
                        }
                        EngineStateError::Deploy => Error::SpeculativeExecution(
                            SpeculativeExecutionError::InvalidDeploy("".into()),
                        ),
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
                        | EngineStateError::FailedToRetrieveEraId => Error::SpeculativeExecution(
                            SpeculativeExecutionError::InternalError(error.to_string()),
                        ),
                        _ => Error::SpeculativeExecution(SpeculativeExecutionError::InternalError(
                            format!("Unhandled engine state error: {}", error),
                        )),
                    }
                    .to_string()
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
        + From<AcceptTransactionRequest>,
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
        + Send,
{
    let protocol_builder = ProtocolBuilder::<1>::with_default_channel_config(
        ChannelConfiguration::default()
            .with_request_limit(3)
            .with_max_request_payload_size(4096)
            .with_max_response_payload_size(4096),
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
        Err(_) => (), // TODO[RC] Silently ignore for now, other node started listening before us,
    };
}

impl<REv> PortBoundComponent<REv> for BinaryPort
where
    REv: From<Event>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<AcceptTransactionRequest>
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
