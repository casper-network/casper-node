use std::{convert::Infallible, time::Duration};

use futures::{future, TryFutureExt};
use hyper::server::{conn::AddrIncoming, Builder};
use tokio::sync::oneshot;
use tower::builder::ServiceBuilder;
use tracing::{info, warn};
use warp::Filter;

use casper_types::ProtocolVersion;

use super::{filters, ReactorEventT};
use crate::effect::EffectBuilder;

/// Run the REST HTTP server.
///
/// A message received on `shutdown_receiver` will cause the server to exit cleanly.
pub(super) async fn run<REv: ReactorEventT>(
    builder: Builder<AddrIncoming>,
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
    shutdown_receiver: oneshot::Receiver<()>,
    qps_limit: u64,
) {
    // REST filters.
    let rest_status = filters::create_status_filter(effect_builder, api_version);
    let rest_metrics = filters::create_metrics_filter(effect_builder);
    let rest_open_rpc = filters::create_rpc_schema_filter(effect_builder);
    let rest_validator_changes =
        filters::create_validator_changes_filter(effect_builder, api_version);
    let rest_chainspec_filter = filters::create_chainspec_filter(effect_builder, api_version);

    let service = warp::service(
        rest_status
            .or(rest_metrics)
            .or(rest_open_rpc)
            .or(rest_validator_changes)
            .or(rest_chainspec_filter)
            .with(warp::cors().allow_any_origin()),
    );

    // Start the server, passing a oneshot receiver to allow the server to be shut down gracefully.
    let make_svc =
        hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));

    let rate_limited_service = ServiceBuilder::new()
        .rate_limit(qps_limit, Duration::from_secs(1))
        .service(make_svc);

    let server = builder.serve(rate_limited_service);
    info!(address = %server.local_addr(), "started REST server");

    // Shutdown the server gracefully.
    let _ = server
        .with_graceful_shutdown(async {
            shutdown_receiver.await.ok();
        })
        .map_err(|error| {
            warn!(%error, "error running REST server");
        })
        .await;
}
