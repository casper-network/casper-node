//! JSON-RPC server
//!
//! The JSON-RPC server provides clients with an API for querying state and
//! sending commands to the node.
//!
//! The actual server is run in backgrounded tasks. RPCs requests are translated into reactor
//! requests to various components.
//!
//! This module currently provides both halves of what is required for an API server:
//! a component implementation that interfaces with other components via being plugged into a
//! reactor, and an external facing http server that exposes various uri routes and converts
//! JSON-RPC requests into the appropriate component events.
//!
//! For the list of supported RPC methods, see:
//! <https://github.com/CasperLabs/ceps/blob/master/text/0009-client-api.md#rpcs>

mod config;
mod event;
mod http_server;
pub mod rpcs;
mod speculative_exec_config;
mod speculative_exec_server;

use std::{convert::Infallible, fmt::Debug, time::Instant};

use datasize::DataSize;
use futures::join;
use tracing::error;

use casper_execution_engine::core::engine_state::{
    self, BalanceRequest, BalanceResult, GetBidsRequest, GetEraValidatorsError, QueryRequest,
    QueryResult,
};
use casper_hashing::Digest;
use casper_types::{system::auction::EraValidators, ExecutionResult, Key, ProtocolVersion, URef};

use self::rpcs::chain::BlockIdentifier;
use super::Component;
use crate::{
    components::{
        contract_runtime::EraValidatorsRequest, ComponentStatus, InitializedComponent,
        PortBoundComponent,
    },
    contract_runtime::SpeculativeExecutionState,
    effect::{
        announcements::RpcServerAnnouncement,
        requests::{
            ChainspecRawBytesRequest, ConsensusRequest, ContractRuntimeRequest, MetricsRequest,
            NetworkInfoRequest, NodeStateRequest, RpcRequest, StorageRequest,
            UpgradeWatcherRequest,
        },
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{BlockHeader, ChainspecInfo, Deploy, StatusFeed},
    utils::{self, ListeningError},
    NodeRng,
};
pub use config::Config;
pub(crate) use event::Event;
pub use speculative_exec_config::Config as SpeculativeExecConfig;

/// A helper trait capturing all of this components Request type dependencies.
pub(crate) trait ReactorEventT:
    From<Event>
    + From<RpcRequest>
    + From<RpcServerAnnouncement>
    + From<ChainspecRawBytesRequest>
    + From<UpgradeWatcherRequest>
    + From<ContractRuntimeRequest>
    + From<ConsensusRequest>
    + From<MetricsRequest>
    + From<NetworkInfoRequest>
    + From<StorageRequest>
    + From<NodeStateRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<RpcRequest>
        + From<RpcServerAnnouncement>
        + From<ChainspecRawBytesRequest>
        + From<UpgradeWatcherRequest>
        + From<ContractRuntimeRequest>
        + From<ConsensusRequest>
        + From<MetricsRequest>
        + From<NetworkInfoRequest>
        + From<StorageRequest>
        + From<NodeStateRequest>
        + Send
        + 'static
{
}

#[derive(DataSize, Debug)]
pub(crate) struct InnerRpcServer {
    /// The instant at which the node has started.
    node_startup_instant: Instant,
    /// The network name, as specified in the chainspec
    network_name: String,
}

impl InnerRpcServer {
    pub fn node_startup_instant(&self) -> Instant {
        self.node_startup_instant
    }

    fn network_name(&self) -> String {
        self.network_name.clone()
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct RpcServer {
    /// The status.
    status: ComponentStatus,
    /// The config.
    config: Config,
    /// The config for speculative execution.
    speculative_exec_config: SpeculativeExecConfig,
    /// The api version.
    api_version: ProtocolVersion,
    /// The network name.
    network_name: String,
    /// The uptime start.
    node_startup_instant: Instant,
    /// Inner JSON-RPC server is present only when enabled in the JSON-RPC
    /// server config.
    inner_rpc: Option<InnerRpcServer>,
    /// Inner speculative execution JSON-RPC server is present only when enabled
    /// in the speculative execution JSON-RPC server config.
    /// The inner speculative execution JSON-RPC server as a struct would have
    /// no fields and no methods because all that is needed to operate it is the
    /// spawned tokio task, so a unit struct will suffice here.
    speculative_exec: Option<()>,
}

impl RpcServer {
    pub(crate) fn new(
        config: Config,
        speculative_exec_config: SpeculativeExecConfig,
        api_version: ProtocolVersion,
        network_name: String,
        node_startup_instant: Instant,
    ) -> Self {
        RpcServer {
            status: ComponentStatus::Uninitialized,
            config,
            speculative_exec_config,
            api_version,
            network_name,
            node_startup_instant,
            inner_rpc: None,
            speculative_exec: None,
        }
    }
}

impl RpcServer {
    fn handle_query<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        state_root_hash: Digest,
        base_key: Key,
        path: Vec<String>,
        responder: Responder<Result<QueryResult, engine_state::Error>>,
    ) -> Effects<Event> {
        let query = QueryRequest::new(state_root_hash, base_key, path);
        effect_builder
            .query_global_state(query)
            .event(move |result| Event::QueryGlobalStateResult {
                result,
                main_responder: responder,
            })
    }

    fn handle_era_validators<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        state_root_hash: Digest,
        protocol_version: ProtocolVersion,
        responder: Responder<Result<EraValidators, GetEraValidatorsError>>,
    ) -> Effects<Event> {
        let request = EraValidatorsRequest::new(state_root_hash, protocol_version);
        effect_builder
            .get_era_validators_from_contract_runtime(request)
            .event(move |result| Event::QueryEraValidatorsResult {
                result,
                main_responder: responder,
            })
    }

    fn handle_get_balance<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        state_root_hash: Digest,
        purse_uref: URef,
        responder: Responder<Result<BalanceResult, engine_state::Error>>,
    ) -> Effects<Event> {
        let query = BalanceRequest::new(state_root_hash, purse_uref);
        effect_builder
            .get_balance(query)
            .event(move |result| Event::GetBalanceResult {
                result,
                main_responder: responder,
            })
    }

    fn handle_execute_deploy<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block_header: BlockHeader,
        deploy: Deploy,
        responder: Responder<Result<Option<ExecutionResult>, engine_state::Error>>,
    ) -> Effects<Event> {
        async move {
            let execution_prestate = SpeculativeExecutionState {
                state_root_hash: *block_header.state_root_hash(),
                block_time: block_header.timestamp(),
                protocol_version: block_header.protocol_version(),
            };
            let result = effect_builder
                .speculative_execute_deploy(execution_prestate, deploy)
                .await;
            responder.respond(result).await
        }
        .ignore()
    }
}

impl<REv> Component<REv> for RpcServer
where
    REv: ReactorEventT,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        // TODO: Fraser, I don't think this is necessary given that no inbound requests will
        // enter via the http_server if the port is not bound, and thus there are no such
        // events ever raised
        // // For all requests other than `SpeculativeDeployExecute`, we return
        // // empty effects if the JSON-RPC server is disabled.
        // let rpc_server = match &self.inner_rpc {
        //     Some(rpc_server) => rpc_server,
        //     None => {
        //         return Effects::new();
        //     }
        // };

        match (self.status.clone(), event) {
            (ComponentStatus::Fatal(msg), _) => {
                error!(
                    msg,
                    "should not handle this event when this component has fatal error"
                );
                return Effects::new();
            }
            (ComponentStatus::Uninitialized, Event::Initialize) => {
                let (effects, status) = self.bind(self.config.enable_server, effect_builder);
                self.status = status;
                effects
            }
            (ComponentStatus::Uninitialized, _) => {
                error!("should not handle this event when component is uninitialized");
                self.status =
                    ComponentStatus::Fatal("attempt to use uninitialized component".to_string());
                return Effects::new();
            }
            (ComponentStatus::Initialized, Event::Initialize) => {
                error!("should not initialize when component is already initialized");
                self.status =
                    ComponentStatus::Fatal("attempt to reinitialize component".to_string());
                return Effects::new();
            }
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::SubmitDeploy { deploy, responder }),
            ) => effect_builder
                .announce_deploy_received(deploy, Some(responder))
                .ignore(),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetBlock {
                    maybe_id: Some(BlockIdentifier::Hash(hash)),
                    only_from_available_block_range,
                    responder,
                }),
            ) => effect_builder
                .get_block_with_metadata_from_storage(hash, only_from_available_block_range)
                .event(move |result| Event::GetBlockResult {
                    maybe_id: Some(BlockIdentifier::Hash(hash)),
                    result: Box::new(result),
                    main_responder: responder,
                }),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetBlock {
                    maybe_id: Some(BlockIdentifier::Height(height)),
                    only_from_available_block_range,
                    responder,
                }),
            ) => effect_builder
                .get_block_at_height_with_metadata_from_storage(
                    height,
                    only_from_available_block_range,
                )
                .event(move |result| Event::GetBlockResult {
                    maybe_id: Some(BlockIdentifier::Height(height)),
                    result: Box::new(result),
                    main_responder: responder,
                }),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetBlock {
                    maybe_id: None,
                    only_from_available_block_range: _, /* Requesting for highest block cannot be
                                                         * restricted by block availability index */
                    responder,
                }),
            ) => effect_builder
                .get_highest_block_with_metadata_from_storage()
                .event(move |result| Event::GetBlockResult {
                    maybe_id: None,
                    result: Box::new(result),
                    main_responder: responder,
                }),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetBlockTransfers {
                    block_hash,
                    responder,
                }),
            ) => effect_builder
                .get_block_transfers_from_storage(block_hash)
                .event(move |result| Event::GetBlockTransfersResult {
                    block_hash,
                    result: Box::new(result),
                    main_responder: responder,
                }),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::QueryGlobalState {
                    state_root_hash,
                    base_key,
                    path,
                    responder,
                }),
            ) => self.handle_query(effect_builder, state_root_hash, base_key, path, responder),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::QueryEraValidators {
                    state_root_hash,
                    protocol_version,
                    responder,
                }),
            ) => self.handle_era_validators(
                effect_builder,
                state_root_hash,
                protocol_version,
                responder,
            ),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetBids {
                    state_root_hash,
                    responder,
                }),
            ) => {
                let get_bids_request = GetBidsRequest::new(state_root_hash);
                effect_builder
                    .get_bids(get_bids_request)
                    .event(move |result| Event::GetBidsResult {
                        result,
                        main_responder: responder,
                    })
            }
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetBalance {
                    state_root_hash,
                    purse_uref,
                    responder,
                }),
            ) => self.handle_get_balance(effect_builder, state_root_hash, purse_uref, responder),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetDeploy {
                    hash,
                    responder,
                    finalized_approvals,
                }),
            ) => effect_builder
                .get_deploy_and_metadata_from_storage(hash)
                .event(move |result| Event::GetDeployResult {
                    hash,
                    result: Box::new(result.map(
                        |(deploy_with_finalized_approvals, metadata_ext)| {
                            if finalized_approvals {
                                (deploy_with_finalized_approvals.into_naive(), metadata_ext)
                            } else {
                                (
                                    deploy_with_finalized_approvals.discard_finalized_approvals(),
                                    metadata_ext,
                                )
                            }
                        },
                    )),
                    main_responder: responder,
                }),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetPeers { responder }),
            ) => effect_builder
                .network_peers()
                .event(move |peers| Event::GetPeersResult {
                    peers,
                    main_responder: responder,
                }),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetStatus { responder }),
            ) => {
                let node_uptime = self.node_startup_instant.elapsed();
                let network_name = self.network_name.clone();
                async move {
                    let (last_added_block, peers, next_upgrade, consensus_status, node_state) = join!(
                        effect_builder.get_highest_block_from_storage(),
                        effect_builder.network_peers(),
                        effect_builder.get_next_upgrade(),
                        effect_builder.consensus_status(),
                        effect_builder.get_node_state()
                    );
                    let status_feed = StatusFeed::new(
                        last_added_block,
                        peers,
                        ChainspecInfo::new(network_name, next_upgrade),
                        consensus_status,
                        node_uptime,
                        node_state,
                    );
                    responder.respond(status_feed).await;
                }
                .ignore()
            }
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::GetAvailableBlockRange { responder }),
            ) => async move {
                responder
                    .respond(
                        effect_builder
                            .get_available_block_range_from_storage()
                            .await,
                    )
                    .await
            }
            .ignore(),
            (
                ComponentStatus::Initialized,
                Event::RpcRequest(RpcRequest::SpeculativeDeployExecute {
                    block_header,
                    deploy,
                    responder,
                }),
            ) => match self.speculative_exec {
                Some(_) => {
                    return self.handle_execute_deploy(
                        effect_builder,
                        block_header,
                        *deploy,
                        responder,
                    );
                }
                None => {
                    return Effects::new();
                }
            },
            (
                ComponentStatus::Initialized,
                Event::GetBlockResult {
                    maybe_id: _,
                    result,
                    main_responder,
                },
            ) => main_responder.respond(*result).ignore(),
            (
                ComponentStatus::Initialized,
                Event::GetBlockTransfersResult {
                    result,
                    main_responder,
                    ..
                },
            ) => main_responder.respond(*result).ignore(),
            (
                ComponentStatus::Initialized,
                Event::QueryGlobalStateResult {
                    result,
                    main_responder,
                },
            ) => main_responder.respond(result).ignore(),
            (
                ComponentStatus::Initialized,
                Event::QueryEraValidatorsResult {
                    result,
                    main_responder,
                },
            ) => main_responder.respond(result).ignore(),
            (
                ComponentStatus::Initialized,
                Event::GetBidsResult {
                    result,
                    main_responder,
                },
            ) => main_responder.respond(result).ignore(),
            (
                ComponentStatus::Initialized,
                Event::GetBalanceResult {
                    result,
                    main_responder,
                },
            ) => main_responder.respond(result).ignore(),
            (
                ComponentStatus::Initialized,
                Event::GetDeployResult {
                    hash: _,
                    result,
                    main_responder,
                },
            ) => main_responder.respond(*result).ignore(),
            (
                ComponentStatus::Initialized,
                Event::GetPeersResult {
                    peers,
                    main_responder,
                },
            ) => main_responder.respond(peers).ignore(),
        }
    }
}

impl<REv> InitializedComponent<REv> for RpcServer
where
    REv: ReactorEventT,
{
    fn status(&self) -> ComponentStatus {
        self.status.clone()
    }
}

impl<REv> PortBoundComponent<REv> for RpcServer
where
    REv: ReactorEventT,
{
    type Error = ListeningError;
    type ComponentEvent = Event;

    fn listen(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Self::ComponentEvent>, Self::Error> {
        // Set the speculative execution HTTP server up first. The speculative
        // execution server can operate independently from the JSON-RPC server,
        // so we save its state before we construct the `RpcServer`.
        self.speculative_exec = if self.speculative_exec_config.enable_server {
            let cfg = &self.speculative_exec_config;
            let builder = utils::start_listening(&cfg.address)?;
            tokio::spawn(speculative_exec_server::run(
                builder,
                effect_builder,
                self.api_version,
                cfg.qps_limit,
                cfg.max_body_bytes,
            ));
            Some(())
        } else {
            None
        };

        let cfg = &self.config;
        let builder = utils::start_listening(&cfg.address)?;
        tokio::spawn(http_server::run(
            builder,
            effect_builder,
            self.api_version,
            cfg.qps_limit,
            cfg.max_body_bytes,
        ));

        let node_startup_instant = self.node_startup_instant;
        let network_name = self.network_name.clone();

        self.inner_rpc = Some(InnerRpcServer {
            node_startup_instant,
            network_name,
        });

        Ok(Effects::new())
    }
}

#[cfg(test)]
mod tests {
    use schemars::schema_for_value;

    use crate::{rpcs::docs::OPEN_RPC_SCHEMA, testing::assert_schema};

    #[test]
    fn schema() {
        // To generate the contents to replace the input JSON file, run the test
        // and print the `actual_schema_string` by uncommenting the `println!`
        // towards the end of the test.
        //
        // ```
        // cargo t components::rpc_server::tests::schema -- --nocapture
        // ```

        let schema_path = format!(
            "{}/../resources/test/rpc_schema_hashing.json",
            env!("CARGO_MANIFEST_DIR")
        );

        assert_schema(schema_path, schema_for_value!(OPEN_RPC_SCHEMA.clone()));
    }
}
