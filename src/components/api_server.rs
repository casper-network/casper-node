//! API server
//!
//! The API server provides clients with a REST-like API for query state and sending commands to the
//! node. The actual server is run in a backgrounded tasks, various requests are translated into
//! reactor-requests to various components.
//!
//! This module currently provides both halves of what is required for an API server: An abstract
//! API Server that handles API requests and an external service endpoint based on HTTP+JSON.

mod config;
mod event;

use std::{net::SocketAddr, str};

use bytes::Bytes;
use rand::Rng;
use tracing::{debug, info, warn};
use warp::{
    body,
    filters::path::Tail,
    http::StatusCode,
    reject::Rejection,
    reply::{self, Json, WithStatus},
    Filter,
};

use super::Component;
use crate::{
    components::storage::Storage,
    crypto::hash::Digest,
    effect::{
        requests::{ApiRequest, StorageRequest},
        Effect, EffectBuilder, EffectExt, Multiple,
    },
    reactor::QueueKind,
    types::{DecodingError, Deploy, DeployHash},
};
pub use config::Config;
pub(crate) use event::Event;

const DEPLOY_API_PATH: &str = "deploy";

pub(crate) struct ApiServer {}

impl ApiServer {
    pub(crate) fn new<REv>(
        config: Config,
        effect_builder: EffectBuilder<REv>,
    ) -> (Self, Multiple<Effect<Event>>)
    where
        REv: From<Event> + From<ApiRequest> + Send,
    {
        let effects = Multiple::new();
        let api_server = ApiServer {};

        tokio::spawn(run_server(config, effect_builder));

        (api_server, effects)
    }
}

impl<REv> Component<REv> for ApiServer
where
    REv: From<StorageRequest<Storage>> + Send,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::ApiRequest(ApiRequest::SubmitDeploy { deploy, responder }) => effect_builder
                .put_deploy(*deploy.clone())
                .event(move |result| Event::PutDeployResult {
                    deploy,
                    result,
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::GetDeploy { hash, responder }) => effect_builder
                .get_deploy(hash)
                .event(move |result| Event::GetDeployResult {
                    hash,
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::PutDeployResult {
                deploy,
                result,
                main_responder,
            } => main_responder
                .respond(result.map_err(|error| (*deploy, error.to_string())))
                .ignore(),
            Event::GetDeployResult {
                hash: _,
                result,
                main_responder,
            } => main_responder.respond(result.ok()).ignore(),
        }
    }
}

/// Run the HTTP server.
async fn run_server<REv>(config: Config, effect_builder: EffectBuilder<REv>)
where
    REv: From<Event> + From<ApiRequest> + Send,
{
    let post_deploy = warp::post()
        .and(warp::path(DEPLOY_API_PATH))
        .and(body::bytes())
        .and_then(move |encoded_deploy| parse_post_request(effect_builder, encoded_deploy));

    let get_deploy = warp::get()
        .and(warp::path(DEPLOY_API_PATH))
        .and(warp::path::tail())
        .and_then(move |hex_digest| parse_get_request(effect_builder, hex_digest));

    let mut server_addr = SocketAddr::from((config.bind_interface, config.bind_port));

    debug!(%server_addr, "starting HTTP server");
    match warp::serve(post_deploy.or(get_deploy)).try_bind_ephemeral(server_addr) {
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
    match warp::serve(post_deploy.or(get_deploy)).try_bind_ephemeral(server_addr) {
        Ok((addr, server_fut)) => {
            info!(%addr, "started HTTP server");
            server_fut.await;
        }
        Err(error) => {
            warn!(%error, "failed to start HTTP server");
        }
    }
}

async fn parse_post_request<REv>(
    effect_builder: EffectBuilder<REv>,
    encoded_deploy: Bytes,
) -> Result<WithStatus<Json>, Rejection>
where
    REv: From<Event> + From<ApiRequest> + Send,
{
    let deploy = match str::from_utf8(encoded_deploy.as_ref())
        .map_err(|_| DecodingError)
        .and_then(Deploy::from_json)
    {
        Ok(deploy) => deploy,
        Err(_error) => {
            info!("failed to put deploy");
            let error_reply = "Failed to parse as JSON-encoded Deploy";
            let json = reply::json(&error_reply);
            return Ok(reply::with_status(json, StatusCode::BAD_REQUEST));
        }
    };

    let reply = effect_builder
        .make_request(
            |responder| ApiRequest::SubmitDeploy {
                deploy: Box::new(deploy),
                responder,
            },
            QueueKind::Api,
        )
        .await;
    let json = reply::json(&reply);
    Ok(reply::with_status(json, StatusCode::OK))
}

async fn parse_get_request<REv>(
    effect_builder: EffectBuilder<REv>,
    hex_digest: Tail,
) -> Result<WithStatus<Json>, Rejection>
where
    REv: From<Event> + From<ApiRequest> + Send,
{
    let digest = match Digest::from_hex(hex_digest.as_str()) {
        Ok(digest) => digest,
        Err(error) => {
            info!(%error, "failed to get deploy");
            let error_reply = format!(
                "Failed to parse '{}' as hex-encoded DeployHash.  Error: {}",
                hex_digest.as_str(),
                error
            );
            let json = reply::json(&error_reply);
            return Ok(reply::with_status(json, StatusCode::BAD_REQUEST));
        }
    };

    let reply = effect_builder
        .make_request(
            move |responder| ApiRequest::GetDeploy {
                hash: DeployHash::new(digest),
                responder,
            },
            QueueKind::Api,
        )
        .await
        .and_then(|deploy| deploy.to_json().ok());
    let json = reply::json(&reply);
    Ok(reply::with_status(json, StatusCode::OK))
}
