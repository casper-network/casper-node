//! API server
//!
//! The API server provides clients with a JSON-RPC API for query state and sending commands to the
//! node. The actual server is run in backgrounded tasks, various requests are translated into
//! reactor-requests to various components.
//!
//! This module currently provides both halves of what is required for an API server: An abstract
//! API Server that handles API requests and an external service endpoint based on HTTP & JSON-RPC.
//!
//! For the list of supported RPCs, see
//! https://github.com/CasperLabs/ceps/blob/master/text/0009-client-api.md#rpcs

mod config;
mod event;
mod rpcs;

use std::{convert::Infallible, fmt::Debug, net::SocketAddr};

use futures::{
    future::{self, Either},
    join,
};
use hyper::Server;
use lazy_static::lazy_static;
use rand::{CryptoRng, Rng};
use semver::Version;
use tracing::{debug, info, warn};
use warp::Filter;

use casper_execution_engine::core::engine_state::{self, QueryRequest, QueryResult};
use casper_types::Key;

use super::Component;
use crate::{
    components::storage::Storage,
    effect::{
        announcements::ApiServerAnnouncement,
        requests::{
            ApiRequest, ContractRuntimeRequest, LinearChainRequest, MetricsRequest,
            NetworkInfoRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, Effects, Responder,
    },
    small_network::NodeId,
    types::{BlockHash, StatusFeed},
};
pub use config::Config;
pub(crate) use event::Event;
use rpcs::{RpcWithOptionalParams, RpcWithParams, RpcWithoutParams};

// TODO - confirm if we want to use the protocol version for this.
lazy_static! {
    static ref CLIENT_API_VERSION: Version = Version::new(1, 0, 0);
}

/// A helper trait whose bounds represent the requirements for a reactor event that `run_server` can
/// work with.
trait ReactorEventT:
    From<Event>
    + From<ApiRequest<NodeId>>
    + From<StorageRequest<Storage>>
    + From<LinearChainRequest<NodeId>>
    + From<ContractRuntimeRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<ApiRequest<NodeId>>
        + From<StorageRequest<Storage>>
        + From<LinearChainRequest<NodeId>>
        + From<ContractRuntimeRequest>
        + Send
        + 'static
{
}

#[derive(Debug)]
pub(crate) struct ApiServer {}

impl ApiServer {
    pub(crate) fn new<REv>(config: Config, effect_builder: EffectBuilder<REv>) -> Self
    where
        REv: From<Event>
            + From<ApiRequest<NodeId>>
            + From<StorageRequest<Storage>>
            + From<LinearChainRequest<NodeId>>
            + From<ContractRuntimeRequest>
            + Send,
    {
        tokio::spawn(run_server(config, effect_builder));
        ApiServer {}
    }
}

/// Run the HTTP server.
async fn run_server<REv: ReactorEventT>(config: Config, effect_builder: EffectBuilder<REv>) {
    let put_deploy = rpcs::account::PutDeploy::create_filter(effect_builder);
    let get_block = rpcs::chain::GetBlock::create_filter(effect_builder);
    let get_item = rpcs::state::GetItem::create_filter(effect_builder);
    let get_deploy = rpcs::info::GetDeploy::create_filter(effect_builder);
    let get_peers = rpcs::info::GetPeers::create_filter(effect_builder);
    let get_status = rpcs::info::GetStatus::create_filter(effect_builder);
    let get_metrics = rpcs::info::GetMetrics::create_filter(effect_builder);

    let service = warp_json_rpc::service(
        put_deploy
            .or(get_block)
            .or(get_item)
            .or(get_deploy)
            .or(get_peers)
            .or(get_status)
            .or(get_metrics),
    );

    let mut server_addr = SocketAddr::from((config.bind_interface, config.bind_port));

    // Try to bind to the user's chosen port, or if that fails, try once to bind to any port then
    // error out if that fails too.
    loop {
        match Server::try_bind(&server_addr) {
            Ok(builder) => {
                let make_svc = hyper::service::make_service_fn(move |_| {
                    future::ok::<_, Infallible>(service.clone())
                });
                let server = builder.serve(make_svc);
                info!(address = %server.local_addr(), "started HTTP server");
                if let Err(error) = server.await {
                    debug!(%error, "error running HTTP server");
                }
                return;
            }
            Err(error) => {
                if server_addr.port() == 0 {
                    warn!(%error, "failed to start HTTP server");
                    return;
                } else {
                    server_addr.set_port(0);
                    debug!(%error, "failed to start HTTP server. retrying on random port");
                }
            }
        }
    }
}

impl ApiServer {
    fn handle_query<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        maybe_hash: Option<BlockHash>,
        base_key: Key,
        path: Vec<String>,
        responder: Responder<Option<Result<QueryResult, engine_state::Error>>>,
    ) -> Effects<Event> {
        async move {
            let maybe_state_hash = if let Some(block_hash) = maybe_hash {
                // Try to get the block from storage.
                Either::Left(effect_builder.get_block_from_storage(block_hash))
            } else {
                // Get the last block fom the linear chain.
                Either::Right(effect_builder.get_last_finalized_block())
            }
            .await
            .map(|block| *block.post_state_hash());

            let state_hash = match maybe_state_hash {
                Some(hash) => hash,
                None => {
                    info!("failed to get block for query");
                    return None;
                }
            };

            let query = QueryRequest::new(state_hash.into(), base_key, path);
            Some(effect_builder.query_global_state(query).await)
        }
        .event(move |result| Event::QueryGlobalStateResult {
            result,
            main_responder: responder,
        })
    }
}

impl<REv, R> Component<REv, R> for ApiServer
where
    REv: From<ApiServerAnnouncement>
        + From<NetworkInfoRequest<NodeId>>
        + From<LinearChainRequest<NodeId>>
        + From<ContractRuntimeRequest>
        + From<MetricsRequest>
        + From<StorageRequest<Storage>>
        + From<Event>
        + From<ApiRequest<NodeId>>
        + Send,
    R: Rng + CryptoRng + ?Sized,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::ApiRequest(ApiRequest::SubmitDeploy { deploy, responder }) => {
                let mut effects = effect_builder.announce_deploy_received(deploy).ignore();
                effects.extend(responder.respond(()).ignore());
                effects
            }
            Event::ApiRequest(ApiRequest::GetBlock {
                maybe_hash: Some(hash),
                responder,
            }) => effect_builder
                .get_block_from_storage(hash)
                .event(move |result| Event::GetBlockResult {
                    maybe_hash: Some(hash),
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::GetBlock {
                maybe_hash: None,
                responder,
            }) => effect_builder
                .get_last_finalized_block()
                .event(move |result| Event::GetBlockResult {
                    maybe_hash: None,
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::QueryGlobalState {
                maybe_hash,
                base_key,
                path,
                responder,
            }) => self.handle_query(effect_builder, maybe_hash, base_key, path, responder),
            Event::ApiRequest(ApiRequest::GetDeploy { hash, responder }) => effect_builder
                .get_deploy_and_metadata_from_storage(hash)
                .event(move |result| Event::GetDeployResult {
                    hash,
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::GetPeers { responder }) => effect_builder
                .network_peers()
                .event(move |peers| Event::GetPeersResult {
                    peers,
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::GetStatus { responder }) => async move {
                let (last_finalized_block, peers) = join!(
                    effect_builder.get_last_finalized_block(),
                    effect_builder.network_peers()
                );
                let status_feed = StatusFeed::new(last_finalized_block, peers);
                debug!("GetStatus --status_feed: {:?}", status_feed);
                responder.respond(status_feed).await;
            }
            .ignore(),
            Event::ApiRequest(ApiRequest::GetMetrics { responder }) => effect_builder
                .get_metrics()
                .event(move |text| Event::GetMetricsResult {
                    text,
                    main_responder: responder,
                }),
            Event::GetBlockResult {
                maybe_hash: _,
                result,
                main_responder,
            } => main_responder.respond(*result).ignore(),
            Event::QueryGlobalStateResult {
                result,
                main_responder,
            } => main_responder.respond(result).ignore(),
            Event::GetDeployResult {
                hash: _,
                result,
                main_responder,
            } => main_responder.respond(*result).ignore(),
            Event::GetPeersResult {
                peers,
                main_responder,
            } => main_responder.respond(peers).ignore(),
            Event::GetMetricsResult {
                text,
                main_responder,
            } => main_responder.respond(text).ignore(),
        }
    }
}
