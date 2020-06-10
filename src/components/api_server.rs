//! API server
//!
//! The API server provides clients with a REST-like API for query state and sending commands to the
//! node. The actual server is run in a backgrounded tasks, various requests are translated into
//! reactor-requests to various components.
//!
//! This module currently provides both halves of what is required for an API server: An abstract
//! API Server that handles API requests and an external service endpoint based on HTTP+JSON.

use std::{
    fmt::{self, Display, Formatter},
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use warp::Filter;

use super::Component;
use crate::{
    effect::{requests::ApiRequest, Effect, EffectBuilder, EffectExt, Multiple, Responder},
    reactor::QueueKind,
};

const DEPLOYS_API_PATH: &str = "deploys";

/// API server configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Interface to bind to. Defaults to loopback address.
    pub bind_interface: IpAddr,

    /// Port to bind to. Use 0 for a random port. Defaults to 0.
    pub bind_port: u16,
}

impl Config {
    /// Creates a default instance for `ApiServer`.
    pub fn new() -> Self {
        Config {
            bind_interface: Ipv4Addr::LOCALHOST.into(),
            bind_port: 0,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}

/// Placeholder deploy.
#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Deploy(u32);

impl Display for Deploy {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

pub(crate) struct ApiServer {
    /// Temporary storage for deploys.
    // TODO: Connect this to the actual storage backend.
    deploys: Vec<Deploy>,
}

// Since our server is so simple, we're cheating in the implementation for now by using the
// `ApiRequest` directly. As soon as actual fetching of deploys happens, a proper `Event` needs
// to be introduced.
pub(crate) type Event = ApiRequest;

impl ApiServer {
    pub(crate) fn new<REv>(
        config: Config,
        effect_builder: EffectBuilder<REv>,
    ) -> (Self, Multiple<Effect<Event>>)
    where
        REv: From<ApiRequest> + Send,
    {
        let effects = Multiple::new();
        let api_server = ApiServer {
            deploys: Vec::new(),
        };

        // Start the actual HTTP server.
        tokio::spawn(run_server(config, effect_builder));

        (api_server, effects)
    }
}

impl<REv> Component<REv> for ApiServer {
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        // TODO: We handle all storage locally on the component. The actual storage should be used.
        match event {
            ApiRequest::GetDeploys { responder } => {
                responder.respond(self.deploys.clone()).ignore()
            }
            ApiRequest::SubmitDeploys { responder, deploys } => {
                self.deploys.extend(deploys.into_iter());
                responder.respond(vec![]).ignore()
            }
        }
    }
}

/// Helper function that JSON encodes a response from a reactor request.
// Note: Also takes care of some otherwise tricky type annotations.
async fn handle_http_request<REv, T, Q, F>(
    effect_builder: EffectBuilder<REv>,
    f: F,
) -> Result<warp::reply::Json, warp::reject::Rejection>
where
    REv: From<ApiRequest> + Send,
    T: Send + 'static + Serialize,
    Q: Into<REv>,
    F: FnOnce(Responder<T>) -> Q,
{
    let rv = effect_builder.make_request(f, QueueKind::Api).await;

    Ok(warp::reply::json(&rv))
}

/// Run the HTTP server.
async fn run_server<REv>(config: Config, effect_builder: EffectBuilder<REv>)
where
    REv: From<ApiRequest> + Send,
{
    let get_deploys = warp::get()
        .and(warp::path(DEPLOYS_API_PATH))
        .and_then(move || {
            handle_http_request(effect_builder, |responder| ApiRequest::GetDeploys {
                responder,
            })
        });

    let post_deploys = warp::post()
        .and(warp::path(DEPLOYS_API_PATH))
        .and(warp::body::json())
        .and_then(move |deploys| {
            handle_http_request(effect_builder, |responder| ApiRequest::SubmitDeploys {
                responder,
                deploys,
            })
        });

    let mut server_addr = SocketAddr::from((config.bind_interface, config.bind_port));

    debug!(%server_addr, "starting HTTP server");
    match warp::serve(get_deploys.or(post_deploys)).try_bind_ephemeral(server_addr) {
        Ok((addr, server_fut)) => {
            info!(%addr, "started HTTP server");
            return server_fut.await;
        }
        Err(error) => {
            if server_addr.port() == 0 {
                warn!(%error, "failed to start HTTP server");
                return;
            } else {
                debug!(%error, "failed to start HTTP server. retrying on random port");
            }
        }
    }

    server_addr.set_port(0);
    match warp::serve(get_deploys.or(post_deploys)).try_bind_ephemeral(server_addr) {
        Ok((addr, server_fut)) => {
            info!(%addr, "started HTTP server");
            server_fut.await;
        }
        Err(error) => {
            warn!(%error, "failed to start HTTP server");
        }
    }
}
