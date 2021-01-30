use std::convert::Infallible;

use futures::future::{self};
use hyper::server::{conn::AddrIncoming, Builder};
use tokio::sync::oneshot;
use tower::builder::ServiceBuilder;
use tracing::{info, trace};
use warp::Filter;

use super::{
    rpcs::{self, RpcWithOptionalParamsExt, RpcWithParamsExt, RpcWithoutParamsExt},
    ReactorEventT,
};
use crate::effect::EffectBuilder;
use std::time::Duration;

/// Run the JSON-RPC server.
pub(super) async fn run<REv: ReactorEventT>(
    builder: Builder<AddrIncoming>,
    effect_builder: EffectBuilder<REv>,
    qps_limit: u64,
) {
    // RPC filters.
    let rpc_put_deploy = rpcs::account::PutDeploy::create_filter(effect_builder);
    let rpc_get_block = rpcs::chain::GetBlock::create_filter(effect_builder);
    let rpc_get_block_transfers = rpcs::chain::GetBlockTransfers::create_filter(effect_builder);
    let rpc_get_state_root_hash = rpcs::chain::GetStateRootHash::create_filter(effect_builder);
    let rpc_get_item = rpcs::state::GetItem::create_filter(effect_builder);
    let rpc_get_balance = rpcs::state::GetBalance::create_filter(effect_builder);
    let rpc_get_deploy = rpcs::info::GetDeploy::create_filter(effect_builder);
    let rpc_get_peers = rpcs::info::GetPeers::create_filter(effect_builder);
    let rpc_get_status = rpcs::info::GetStatus::create_filter(effect_builder);
    let rpc_get_era_info = rpcs::chain::GetEraInfoBySwitchBlock::create_filter(effect_builder);
    let rpc_get_auction_info = rpcs::state::GetAuctionInfo::create_filter(effect_builder);
    let rpc_get_rpcs = rpcs::docs::ListRpcs::create_filter(effect_builder);

    let service = warp_json_rpc::service(
        rpc_put_deploy
            .or(rpc_get_block)
            .or(rpc_get_block_transfers)
            .or(rpc_get_state_root_hash)
            .or(rpc_get_item)
            .or(rpc_get_balance)
            .or(rpc_get_deploy)
            .or(rpc_get_peers)
            .or(rpc_get_status)
            .or(rpc_get_era_info)
            .or(rpc_get_auction_info)
            .or(rpc_get_rpcs),
    );

    // Start the server, passing a oneshot receiver to allow the server to be shut down gracefully.
    let make_svc =
        hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));

    let make_svc = ServiceBuilder::new()
        .rate_limit(qps_limit, Duration::from_secs(1))
        .service(make_svc);

    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();

    let server = builder.serve(make_svc);
    info!(address = %server.local_addr(), "started JSON-RPC server");

    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });

    let server_joiner = tokio::spawn(server_with_shutdown);

    let _ = server_joiner.await;

    // Shut down the server.
    let _ = shutdown_sender.send(());

    trace!("JSON-RPC server stopped");
}
