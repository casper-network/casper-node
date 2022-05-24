use std::{convert::Infallible, time::Duration};

use http::header::ACCEPT_ENCODING;
use hyper::server::{conn::AddrIncoming, Builder};
use tokio::sync::oneshot;
use tower::builder::ServiceBuilder;
use tracing::{info, trace};
use warp::Filter;

use casper_json_rpc::RequestHandlersBuilder;
use casper_types::ProtocolVersion;

use super::{
    rpcs::{
        account::PutDeploy,
        chain::{GetBlock, GetBlockTransfers, GetEraInfoBySwitchBlock, GetStateRootHash},
        docs::ListRpcs,
        info::{GetChainspec, GetDeploy, GetPeers, GetStatus, GetValidatorChanges},
        state::{
            GetAccountInfo, GetAuctionInfo, GetBalance, GetDictionaryItem, GetItem, GetTrie,
            QueryBalance, QueryGlobalState,
        },
        RpcWithOptionalParams, RpcWithParams, RpcWithoutParams,
    },
    ReactorEventT, RPC_API_PATH,
};
use crate::effect::EffectBuilder;

/// Run the JSON-RPC server.
pub(super) async fn run<REv: ReactorEventT>(
    builder: Builder<AddrIncoming>,
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
    qps_limit: u64,
    max_body_bytes: u32,
) {
    let mut handlers = RequestHandlersBuilder::new();
    PutDeploy::register_as_handler(effect_builder, api_version, &mut handlers);
    GetBlock::register_as_handler(effect_builder, api_version, &mut handlers);
    GetBlockTransfers::register_as_handler(effect_builder, api_version, &mut handlers);
    GetStateRootHash::register_as_handler(effect_builder, api_version, &mut handlers);
    GetItem::register_as_handler(effect_builder, api_version, &mut handlers);
    QueryGlobalState::register_as_handler(effect_builder, api_version, &mut handlers);
    GetBalance::register_as_handler(effect_builder, api_version, &mut handlers);
    GetAccountInfo::register_as_handler(effect_builder, api_version, &mut handlers);
    GetDeploy::register_as_handler(effect_builder, api_version, &mut handlers);
    GetPeers::register_as_handler(effect_builder, api_version, &mut handlers);
    GetStatus::register_as_handler(effect_builder, api_version, &mut handlers);
    GetEraInfoBySwitchBlock::register_as_handler(effect_builder, api_version, &mut handlers);
    GetAuctionInfo::register_as_handler(effect_builder, api_version, &mut handlers);
    GetTrie::register_as_handler(effect_builder, api_version, &mut handlers);
    GetValidatorChanges::register_as_handler(effect_builder, api_version, &mut handlers);
    ListRpcs::register_as_handler(effect_builder, api_version, &mut handlers);
    GetDictionaryItem::register_as_handler(effect_builder, api_version, &mut handlers);
    GetChainspec::register_as_handler(effect_builder, api_version, &mut handlers);
    QueryBalance::register_as_handler(effect_builder, api_version, &mut handlers);
    let handlers = handlers.build();

    let make_svc = hyper::service::make_service_fn(move |_| {
        let service_routes = casper_json_rpc::route(RPC_API_PATH, max_body_bytes, handlers.clone());

        // Supports content negotiation for gzip responses. This is an interim fix until
        // https://github.com/seanmonstar/warp/pull/513 moves forward.
        let service_routes_gzip = warp::header::exact(ACCEPT_ENCODING.as_str(), "gzip")
            .and(service_routes.clone())
            .with(warp::compression::gzip());

        let service = warp::service(service_routes_gzip.or(service_routes));
        async move { Ok::<_, Infallible>(service.clone()) }
    });

    let make_svc = ServiceBuilder::new()
        .rate_limit(qps_limit, Duration::from_secs(1))
        .service(make_svc);

    let server = builder.serve(make_svc);
    info!(address = %server.local_addr(), "started JSON-RPC server");

    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();
    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });

    let _ = tokio::spawn(server_with_shutdown).await;
    let _ = shutdown_sender.send(());
    trace!("JSON-RPC server stopped");
}
