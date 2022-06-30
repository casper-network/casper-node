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
    components::contract_runtime::EraValidatorsRequest,
    contract_runtime::SpeculativeExecutionState,
    effect::{
        announcements::RpcServerAnnouncement,
        requests::{
            ChainspecLoaderRequest, ConsensusRequest, ContractRuntimeRequest, MetricsRequest,
            NetworkInfoRequest, RpcRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{BlockHeader, Deploy, NodeState, StatusFeed},
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
    + From<ChainspecLoaderRequest>
    + From<ContractRuntimeRequest>
    + From<ConsensusRequest>
    + From<MetricsRequest>
    + From<NetworkInfoRequest>
    + From<StorageRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<RpcRequest>
        + From<RpcServerAnnouncement>
        + From<ChainspecLoaderRequest>
        + From<ContractRuntimeRequest>
        + From<ConsensusRequest>
        + From<MetricsRequest>
        + From<NetworkInfoRequest>
        + From<StorageRequest>
        + Send
        + 'static
{
}

#[derive(DataSize, Debug)]
pub(crate) struct RpcServer {
    /// Indicates whether the JSON-RPC server is enabled or not.
    enable: bool,
    /// Indicates whether the speculative execution JSON-RPC server is enabled or not.
    speculative_exec_enable: bool,
    /// The instant at which the node has started.
    node_startup_instant: Instant,
    /// The current state of the node.
    node_state: NodeState,
}

impl RpcServer {
    pub(crate) fn new<REv>(
        config: Config,
        speculative_exec_config: SpeculativeExecConfig,
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        node_startup_instant: Instant,
        node_state: NodeState,
    ) -> Result<Self, ListeningError>
    where
        REv: ReactorEventT,
    {
        if config.enable_server {
            let builder = utils::start_listening(&config.address)?;
            tokio::spawn(http_server::run(
                builder,
                effect_builder,
                api_version,
                config.qps_limit,
                config.max_body_bytes,
            ));
        }

        // Set the speculative execution HTTP server up.
        if speculative_exec_config.enable_server {
            let builder = utils::start_listening(&speculative_exec_config.address)?;
            tokio::spawn(speculative_exec_server::run(
                builder,
                effect_builder,
                api_version,
                speculative_exec_config.qps_limit,
                speculative_exec_config.max_body_bytes,
            ));
        }

        Ok(RpcServer {
            enable: config.enable_server,
            speculative_exec_enable: speculative_exec_config.enable_server,
            node_startup_instant,
            node_state,
        })
    }

    fn node_state(&self) -> NodeState {
        self.node_state
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
        match event {
            Event::RpcRequest(RpcRequest::SubmitDeploy { deploy, responder }) if self.enable => {
                effect_builder
                    .announce_deploy_received(deploy, Some(responder))
                    .ignore()
            }
            Event::RpcRequest(RpcRequest::GetBlock {
                maybe_id: Some(BlockIdentifier::Hash(hash)),
                only_from_available_block_range,
                responder,
            }) if self.enable => effect_builder
                .get_block_with_metadata_from_storage(hash, only_from_available_block_range)
                .event(move |result| Event::GetBlockResult {
                    maybe_id: Some(BlockIdentifier::Hash(hash)),
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::RpcRequest(RpcRequest::GetBlock {
                maybe_id: Some(BlockIdentifier::Height(height)),
                only_from_available_block_range,
                responder,
            }) if self.enable => effect_builder
                .get_block_at_height_with_metadata_from_storage(
                    height,
                    only_from_available_block_range,
                )
                .event(move |result| Event::GetBlockResult {
                    maybe_id: Some(BlockIdentifier::Height(height)),
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::RpcRequest(RpcRequest::GetBlock {
                maybe_id: None,
                only_from_available_block_range: _, /* Requesting for higest block cannot be
                                                     * restricted by block availability index */
                responder,
            }) if self.enable => effect_builder
                .get_highest_block_with_metadata_from_storage()
                .event(move |result| Event::GetBlockResult {
                    maybe_id: None,
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::RpcRequest(RpcRequest::GetBlockTransfers {
                block_hash,
                responder,
            }) if self.enable => effect_builder
                .get_block_transfers_from_storage(block_hash)
                .event(move |result| Event::GetBlockTransfersResult {
                    block_hash,
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::RpcRequest(RpcRequest::QueryGlobalState {
                state_root_hash,
                base_key,
                path,
                responder,
            }) if self.enable => {
                self.handle_query(effect_builder, state_root_hash, base_key, path, responder)
            }
            Event::RpcRequest(RpcRequest::QueryEraValidators {
                state_root_hash,
                protocol_version,
                responder,
            }) if self.enable => self.handle_era_validators(
                effect_builder,
                state_root_hash,
                protocol_version,
                responder,
            ),
            Event::RpcRequest(RpcRequest::GetBids {
                state_root_hash,
                responder,
            }) if self.enable => {
                let get_bids_request = GetBidsRequest::new(state_root_hash);
                effect_builder
                    .get_bids(get_bids_request)
                    .event(move |result| Event::GetBidsResult {
                        result,
                        main_responder: responder,
                    })
            }
            Event::RpcRequest(RpcRequest::GetBalance {
                state_root_hash,
                purse_uref,
                responder,
            }) if self.enable => {
                self.handle_get_balance(effect_builder, state_root_hash, purse_uref, responder)
            }
            Event::RpcRequest(RpcRequest::GetDeploy {
                hash,
                responder,
                finalized_approvals,
            }) if self.enable => effect_builder
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
            Event::RpcRequest(RpcRequest::GetPeers { responder }) if self.enable => effect_builder
                .network_peers()
                .event(move |peers| Event::GetPeersResult {
                    peers,
                    main_responder: responder,
                }),
            Event::RpcRequest(RpcRequest::GetStatus { responder }) if self.enable => {
                let node_uptime = self.node_startup_instant.elapsed();
                let node_state = self.node_state();
                async move {
                    let (last_added_block, peers, chainspec_info, consensus_status) = join!(
                        effect_builder.get_highest_block_from_storage(),
                        effect_builder.network_peers(),
                        effect_builder.get_chainspec_info(),
                        effect_builder.consensus_status()
                    );
                    let status_feed = StatusFeed::new(
                        last_added_block,
                        peers,
                        chainspec_info,
                        consensus_status,
                        node_uptime,
                        node_state,
                    );
                    responder.respond(status_feed).await;
                }
                .ignore()
            }
            Event::RpcRequest(RpcRequest::GetAvailableBlockRange { responder }) if self.enable => {
                async move {
                    responder
                        .respond(
                            effect_builder
                                .get_available_block_range_from_storage()
                                .await,
                        )
                        .await
                }
                .ignore()
            }
            Event::RpcRequest(RpcRequest::SpeculativeDeployExecute {
                block_header,
                deploy,
                responder,
            }) if self.speculative_exec_enable => {
                self.handle_execute_deploy(effect_builder, block_header, *deploy, responder)
            }
            Event::GetBlockResult {
                maybe_id: _,
                result,
                main_responder,
            } if self.enable => main_responder.respond(*result).ignore(),
            Event::GetBlockTransfersResult {
                result,
                main_responder,
                ..
            } if self.enable => main_responder.respond(*result).ignore(),
            Event::QueryGlobalStateResult {
                result,
                main_responder,
            } if self.enable => main_responder.respond(result).ignore(),
            Event::QueryEraValidatorsResult {
                result,
                main_responder,
            } if self.enable => main_responder.respond(result).ignore(),
            Event::GetBidsResult {
                result,
                main_responder,
            } if self.enable => main_responder.respond(result).ignore(),
            Event::GetBalanceResult {
                result,
                main_responder,
            } if self.enable => main_responder.respond(result).ignore(),
            Event::GetDeployResult {
                hash: _,
                result,
                main_responder,
            } if self.enable => main_responder.respond(*result).ignore(),
            Event::GetPeersResult {
                peers,
                main_responder,
            } if self.enable => main_responder.respond(peers).ignore(),
            Event::RpcRequest(RpcRequest::SpeculativeDeployExecute { .. })
                if !self.speculative_exec_enable =>
            {
                Effects::new()
            }
            Event::RpcRequest(_)
            | Event::GetBalanceResult { .. }
            | Event::GetBidsResult { .. }
            | Event::GetBlockResult { .. }
            | Event::GetBlockTransfersResult { .. }
            | Event::GetDeployResult { .. }
            | Event::GetPeersResult { .. }
            | Event::QueryEraValidatorsResult { .. }
            | Event::QueryGlobalStateResult { .. }
                if !self.enable =>
            {
                Effects::new()
            }
            ev => {
                error!("Unhandled event in JSON-RPC server: {}", ev);
                Effects::new()
            }
        }
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
