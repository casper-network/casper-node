use std::convert::Infallible;

use futures::future::{self};
use hyper::Server;
use tokio::sync::oneshot;
use tracing::{debug, info, trace, warn};
use warp::Filter;

use super::{
    rpcs::{self, RpcWithOptionalParamsExt, RpcWithParamsExt, RpcWithoutParamsExt},
    Config, ReactorEventT,
};
use crate::{effect::EffectBuilder, utils};

/// Run the HTTP server.
///
/// `data_receiver` will provide the server with local events which should then be sent to all
/// subscribed clients.
pub(super) async fn run<REv: ReactorEventT>(config: Config, effect_builder: EffectBuilder<REv>) {
    // RPC filters.
    let rpc_put_deploy = rpcs::account::PutDeploy::create_filter(effect_builder);
    let rpc_get_block = rpcs::chain::GetBlock::create_filter(effect_builder);
    let rpc_get_state_root_hash = rpcs::chain::GetStateRootHash::create_filter(effect_builder);
    let rpc_get_item = rpcs::state::GetItem::create_filter(effect_builder);
    let rpc_get_balance = rpcs::state::GetBalance::create_filter(effect_builder);
    let rpc_get_deploy = rpcs::info::GetDeploy::create_filter(effect_builder);
    let rpc_get_peers = rpcs::info::GetPeers::create_filter(effect_builder);
    let rpc_get_status = rpcs::info::GetStatus::create_filter(effect_builder);
    let rpc_get_auction_info = rpcs::state::GetAuctionInfo::create_filter(effect_builder);

    let service = warp_json_rpc::service(
        rpc_put_deploy
            .or(rpc_get_block)
            .or(rpc_get_state_root_hash)
            .or(rpc_get_item)
            .or(rpc_get_balance)
            .or(rpc_get_deploy)
            .or(rpc_get_peers)
            .or(rpc_get_status)
            .or(rpc_get_auction_info),
    );

    let mut server_address = match utils::resolve_address(&config.address) {
        Ok(address) => address,
        Err(error) => {
            warn!(%error, "failed to start HTTP server, cannot parse address");
            return;
        }
    };

    // Try to bind to the user's chosen port, or if that fails, try once to bind to any port then
    // error out if that fails too.
    let builder = loop {
        match Server::try_bind(&server_address) {
            Ok(builder) => {
                break builder;
            }
            Err(error) => {
                if server_address.port() == 0 {
                    warn!(%error, "failed to start HTTP server");
                    return;
                } else {
                    server_address.set_port(0);
                    debug!(%error, "failed to start HTTP server. retrying on random port");
                }
            }
        }
    };

    // Start the server, passing a oneshot receiver to allow the server to be shut down gracefully.
    let make_svc =
        hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));
    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();

    let server = builder.serve(make_svc);
    info!(address = %server.local_addr(), "started RPC server");

    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });

    let server_joiner = tokio::spawn(server_with_shutdown);

    let _ = server_joiner.await;

    // Kill the event-stream handlers, and shut down the server.
    let _ = shutdown_sender.send(());

    trace!("RPC server stopped");
}
