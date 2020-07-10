//! API server
//!
//! The API server provides clients with a REST-like API for query state and sending commands to the
//! node. The actual server is run in a backgrounded tasks, various requests are translated into
//! reactor-requests to various components.
//!
//! This module currently provides both halves of what is required for an API server: An abstract
//! API Server that handles API requests and an external service endpoint based on HTTP+JSON.
//!
//! API
//! * To store a deploy, send an HTTP POST request to "/deploys" where the body is the
//!   JSON-serialized deploy.  The response will be the deploy's hash (hex-encoded) or an error
//!   message on failure.
//! * To retrieve a deploy, send an HTTP GET request to "/deploys/<ID>" where <ID> is the
//!   hex-encoded deploy hash.  The response will be the JSON-serialized deploy, "null"  if the
//!   deploy doesn't exist or an error message on failure..
//! * To list all stored deploy hashes, send an HTTP GET request to "/deploys".  The response will
//!   be the JSON-serialized list of hex-encoded deploy hashes or an error message on failure.

mod config;
mod event;

use std::{error::Error as StdError, net::SocketAddr, str};

use bytes::Bytes;
use http::Response;
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
    components::storage::{self, Storage},
    crypto::hash::Digest,
    effect::{
        requests::{ApiRequest, ContractRuntimeRequest, DeployGossiperRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    reactor::QueueKind,
    types::{DecodingError, Deploy, DeployHash},
};
pub use config::Config;
pub(crate) use event::Event;

const DEPLOYS_API_PATH: &str = "deploys";

#[derive(Debug)]
pub(crate) struct ApiServer {}

impl ApiServer {
    pub(crate) fn new<REv>(config: Config, effect_builder: EffectBuilder<REv>) -> Self
    where
        REv: From<Event> + From<ApiRequest> + From<StorageRequest<Storage>> + Send,
    {
        tokio::spawn(run_server(config, effect_builder));
        ApiServer {}
    }
}

impl<REv> Component<REv> for ApiServer
where
    REv: From<StorageRequest<Storage>>
        + From<DeployGossiperRequest>
        + From<ContractRuntimeRequest>
        + Send,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::ApiRequest(ApiRequest::SubmitDeploy { deploy, responder }) => effect_builder
                .put_deploy_to_storage(*deploy.clone())
                .event(move |result| Event::PutDeployResult {
                    deploy,
                    result,
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::GetDeploy { hash, responder }) => effect_builder
                .get_deploy_from_storage(hash)
                .event(move |result| Event::GetDeployResult {
                    hash,
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::ListDeploys { responder }) => effect_builder
                .list_deploys()
                .event(move |result| Event::ListDeploysResult {
                    result: Box::new(result),
                    main_responder: responder,
                }),
            Event::PutDeployResult {
                deploy,
                result,
                main_responder,
            } => {
                let cloned_deploy = deploy.clone();
                let mut effects = main_responder
                    .respond(result.map_err(|error| (*deploy, error)))
                    .ignore();
                effects.extend(effect_builder.gossip_deploy(cloned_deploy).ignore());
                effects
            }
            Event::GetDeployResult {
                hash: _,
                result,
                main_responder,
            } => main_responder.respond(*result).ignore(),
            Event::ListDeploysResult {
                result,
                main_responder,
            } => main_responder.respond(*result).ignore(),
        }
    }
}

/// Run the HTTP server.
async fn run_server<REv>(config: Config, effect_builder: EffectBuilder<REv>)
where
    REv: From<Event> + From<ApiRequest> + From<StorageRequest<Storage>> + Send,
{
    let post_deploy = warp::post()
        .and(warp::path(DEPLOYS_API_PATH))
        .and(body::bytes())
        .and_then(move |encoded_deploy| parse_post_request(effect_builder, encoded_deploy));

    let get_deploy = warp::get()
        .and(warp::path(DEPLOYS_API_PATH))
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

    let result = effect_builder
        .make_request(
            |responder| ApiRequest::SubmitDeploy {
                deploy: Box::new(deploy),
                responder,
            },
            QueueKind::Api,
        )
        .await;

    match result {
        Ok(()) => {
            let json = reply::json(&"");
            Ok(reply::with_status(json, StatusCode::OK))
        }
        Err((deploy, error)) => {
            let error_reply = format!("Failed to store {}: {}", deploy.id(), error);
            let json = reply::json(&error_reply);
            Ok(reply::with_status(json, StatusCode::BAD_REQUEST))
        }
    }
}

async fn parse_get_request<REv>(
    effect_builder: EffectBuilder<REv>,
    tail: Tail,
) -> Result<Response<String>, Rejection>
where
    REv: From<Event> + From<ApiRequest> + From<StorageRequest<Storage>> + Send,
{
    if tail.as_str().is_empty() {
        handle_list_deploys_request(effect_builder).await
    } else {
        handle_get_deploy_request(effect_builder, tail).await
    }
}

async fn handle_list_deploys_request<REv>(
    effect_builder: EffectBuilder<REv>,
) -> Result<Response<String>, Rejection>
where
    REv: From<Event> + From<ApiRequest> + From<StorageRequest<Storage>> + Send,
{
    let result = effect_builder
        .make_request(
            |responder| ApiRequest::ListDeploys { responder },
            QueueKind::Api,
        )
        .await;
    let error_body = |error: &dyn std::error::Error| -> String {
        format!(
            r#""Internal server error listing deploys.  Error: {}""#,
            error
        )
    };

    // TODO - paginate these?
    let (body, status) = match result {
        Ok(deploy_hashes) => {
            let hex_hashes = deploy_hashes
                .into_iter()
                .map(|deploy_hash| hex::encode(deploy_hash.inner()))
                .collect::<Vec<_>>();
            match serde_json::to_string(&hex_hashes) {
                Ok(body) => (body, StatusCode::OK),
                Err(error) => (error_body(&error), StatusCode::INTERNAL_SERVER_ERROR),
            }
        }
        Err(error) => (error_body(&error), StatusCode::INTERNAL_SERVER_ERROR),
    };

    Ok(Response::builder()
        .header("content-type", "application/json")
        .status(status)
        .body(body)
        .unwrap())
}

async fn handle_get_deploy_request<REv>(
    effect_builder: EffectBuilder<REv>,
    hex_digest: Tail,
) -> Result<Response<String>, Rejection>
where
    REv: From<Event> + From<ApiRequest> + From<StorageRequest<Storage>> + Send,
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
            let response = Response::builder()
                .header("content-type", "application/json")
                .status(StatusCode::BAD_REQUEST)
                .body(error_reply)
                .unwrap();
            return Ok(response);
        }
    };

    let result = effect_builder
        .make_request(
            |responder| ApiRequest::GetDeploy {
                hash: DeployHash::new(digest),
                responder,
            },
            QueueKind::Api,
        )
        .await;

    let error_body = |error: &dyn StdError| -> String {
        format!(
            r#""Internal server error retrieving {}.  Error: {}""#,
            hex_digest.as_str(),
            error
        )
    };

    let (body, status) = match result {
        Ok(deploy) => match deploy.to_json() {
            Ok(deploy_as_json) => (deploy_as_json, StatusCode::OK),
            Err(error) => (error_body(&error), StatusCode::INTERNAL_SERVER_ERROR),
        },
        Err(storage::Error::NotFound) => ("null".to_string(), StatusCode::OK),
        Err(error) => (error_body(&error), StatusCode::INTERNAL_SERVER_ERROR),
    };

    Ok(Response::builder()
        .header("content-type", "application/json")
        .status(status)
        .body(body)
        .unwrap())
}
