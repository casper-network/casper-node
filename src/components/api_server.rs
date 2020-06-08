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
    net::SocketAddr,
};

use serde::{Deserialize, Serialize};
use tracing::info;
use warp::Filter;

use super::Component;
use crate::{
    effect::{requests::ApiRequest, Effect, EffectBuilder, EffectExt, Multiple, Responder},
    reactor::QueueKind,
};

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

impl<REv> Component<REv> for ApiServer {
    // Since our server is so simple, we're cheating in the implementation for now by using the
    // `ApiRequest` directly. As soon as actual fetching of deploys happens, a proper `Event` needs
    // to be introduced.
    type Event = ApiRequest;
    fn handle_event(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
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

/// Helper function that JSON encodes a response from a reacter request.
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
async fn run_server<REv>(server_addr: SocketAddr, effect_builder: EffectBuilder<REv>)
where
    REv: From<ApiRequest> + Send,
{
    let get_deploys = warp::get().and(warp::path("deploys")).and_then(move || {
        handle_http_request(effect_builder, |responder| ApiRequest::GetDeploys {
            responder,
        })
    });

    let post_deploys = warp::post()
        .and(warp::path("deploys"))
        .and(warp::body::json())
        .and_then(move |deploys| {
            handle_http_request(effect_builder, |responder| ApiRequest::SubmitDeploys {
                responder,
                deploys,
            })
        });

    info!(%server_addr, "starting HTTP server");

    // TODO: This call will panic if the port cannot be bound. It is possible that a framework other
    //       than `warp` is a better choice overall.
    warp::serve(get_deploys.or(post_deploys))
        .run(server_addr)
        .await;
}
