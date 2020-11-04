use std::convert::Infallible;

use futures::{future, TryFutureExt};
use hyper::Server;
use tokio::sync::oneshot;
use tracing::{debug, info, warn};
use warp::Filter;

use super::{filters, Config, ReactorEventT};
use crate::{effect::EffectBuilder, utils};

/// Run the HTTP server.
///
/// A message received on `shutdown_receiver` will cause the server to exit cleanly.
pub(super) async fn run<REv: ReactorEventT>(
    config: Config,
    effect_builder: EffectBuilder<REv>,
    shutdown_receiver: oneshot::Receiver<()>,
) {
    // REST filters.
    let rest_status = filters::create_status_filter(effect_builder);
    let rest_metrics = filters::create_metrics_filter(effect_builder);

    let service = warp_json_rpc::service(rest_status.or(rest_metrics));

    let mut server_address = match utils::resolve_address(&config.address) {
        Ok(address) => address,
        Err(error) => {
            warn!(%error, "failed to start REST server, cannot parse address");
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
                    warn!(%error, "failed to start REST server");
                    return;
                } else {
                    server_address.set_port(0);
                    debug!(%error, "failed to start REST server. retrying on random port");
                }
            }
        }
    };

    // Start the server, passing a oneshot receiver to allow the server to be shut down gracefully.
    let make_svc =
        hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));

    let server = builder.serve(make_svc);
    info!(address = %server.local_addr(), "started REST server");

    let _ = server
        .with_graceful_shutdown(async {
            shutdown_receiver.await.ok();
        })
        .map_err(|error| {
            warn!(%error, "error running rest server");
        })
        .await;
}
