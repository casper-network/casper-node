use std::{convert::Infallible, time::Duration};

use http::header::ACCEPT_ENCODING;
use hyper::server::{conn::AddrIncoming, Builder};
use tokio::sync::oneshot;
use tower::ServiceBuilder;
use tracing::info;

use casper_json_rpc::RequestHandlersBuilder;
use casper_types::ProtocolVersion;
use warp::Filter;

use super::ReactorEventT;
use crate::effect::EffectBuilder;

/// The URL path for all JSON-RPC requests.
pub const SPECULATIVE_EXEC_API_PATH: &str = "rpc";

/// Run the speculative execution server.
pub(super) async fn run<REv: ReactorEventT>(
    builder: Builder<AddrIncoming>,
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
    qps_limit: u64,
    max_body_bytes: u32,
) {
    let mut handlers = RequestHandlersBuilder::new();
    let handlers = handlers.build();

    let make_svc = hyper::service::make_service_fn(move |_| {
        let service_routes =
            casper_json_rpc::route(SPECULATIVE_EXEC_API_PATH, max_body_bytes, handlers.clone());

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
    info!(address = %server.local_addr(), "started speculative execution server");

    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();
    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });

    let _ = tokio::spawn(server_with_shutdown).await;
    let _ = shutdown_sender.send(());
}
