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

use std::{
    borrow::Cow,
    error::Error as StdError,
    fmt::Debug,
    net::SocketAddr,
    str,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};

use bytes::Bytes;
use futures::{join, FutureExt};
use http::Response;
use rand::{CryptoRng, Rng};
use smallvec::smallvec;
use tracing::{debug, error, info, warn};
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
        announcements::ApiServerAnnouncement,
        requests::{
            ApiRequest, ContractRuntimeRequest, LinearChainRequest, MetricsRequest,
            NetworkInfoRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    reactor::QueueKind,
    small_network::NodeId,
    types::{Deploy, DeployHash, StatusFeed},
};
pub use config::Config;
pub(crate) use event::Event;

const DEPLOYS_API_PATH: &str = "deploys";
const METRICS_API_PATH: &str = "metrics";
const STATUS_API_PATH: &str = "status";

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

/// A cloneable handle to a ticket counter.
#[derive(Clone, Debug)]
struct TicketCounterHandle(Arc<AtomicI64>);

impl TicketCounterHandle {
    /// Creates a new ticket counter and returns a handle to it.
    fn create_counter() -> Self {
        TicketCounterHandle(Arc::new(AtomicI64::new(0)))
    }

    /// Tries to acquire a ticket, returning true on success.
    fn acquire_ticket(&self) -> bool {
        if self.0.fetch_sub(1, Ordering::SeqCst) <= 0 {
            self.0.fetch_add(1, Ordering::SeqCst);
            false
        } else {
            true
        }
    }

    /// Increase the amount of available tickets by `amount`, up to `limit`.
    fn add_tickets(&self, amount: i64, limit: i64) {
        // Add the desired number of tickets.
        let prev = self.0.fetch_add(amount, Ordering::SeqCst);

        // If we overshoot, correct, possibly going negative.
        let overshoot = (prev + amount) - limit;

        if overshoot > 0 {
            self.0.fetch_sub(overshoot, Ordering::SeqCst);
        }
    }
}

/// A background task that increases a ticket counter periodically.
#[derive(Debug)]
struct TicketServer {
    counter: TicketCounterHandle,
    rate_limit: i64,
    burst_limit: i64,
}

impl TicketServer {
    /// Creates a new ticket server from the given configuration.
    fn new(counter: TicketCounterHandle, config: &Config) -> Self {
        TicketServer {
            counter,
            rate_limit: config.accepted_deploy_rate_limit as i64,
            burst_limit: config.accepted_deploy_burst_limit as i64,
        }
    }

    /// Runs the ticket server task.
    async fn run(self) {
        // We opt to keep it running indefinitely like the warp server.
        loop {
            // Wait for a second, then add more tickets.
            tokio::time::delay_for(Duration::from_secs(1)).await;

            self.counter.add_tickets(self.rate_limit, self.burst_limit);
        }
    }
}

/// Run the HTTP server.
async fn run_server<REv>(config: Config, effect_builder: EffectBuilder<REv>)
where
    REv: From<Event> + From<ApiRequest> + From<StorageRequest<Storage>> + Send,
{
    let ticket_counter = TicketCounterHandle::create_counter();

    // Spawn a ticket increasing task.
    let ticket_server = TicketServer::new(ticket_counter.clone(), &config);
    tokio::spawn(ticket_server.run());

    let post_deploy = warp::post()
        .and(warp::path(DEPLOYS_API_PATH))
        .and(body::bytes())
        .and_then(move |encoded_deploy| {
            parse_post_deploy_request(effect_builder, encoded_deploy, ticket_counter.clone())
        });

    let get_deploy = warp::get()
        .and(warp::path(DEPLOYS_API_PATH))
        .and(warp::path::tail())
        .and_then(move |hex_digest| parse_get_deploy_request(effect_builder, hex_digest));

    let get_metrics = warp::get()
        .and(warp::path(METRICS_API_PATH))
        .and_then(move || {
            effect_builder
                .make_request(
                    |responder| ApiRequest::GetMetrics { responder },
                    QueueKind::Api,
                )
                .map(|text_opt| match text_opt {
                    Some(text) => {
                        Ok::<_, Rejection>(reply::with_status(Cow::from(text), StatusCode::OK))
                    }
                    None => Ok(reply::with_status(
                        Cow::from("failed to collect metrics. sorry!"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )),
                })
        });

    let get_status = warp::get()
        .and(warp::path(STATUS_API_PATH))
        .and_then(move || handle_get_status(effect_builder));

    let mut server_addr = SocketAddr::from((config.bind_interface, config.bind_port));

    let filter = post_deploy.or(get_deploy).or(get_metrics).or(get_status);

    debug!(%server_addr, "starting HTTP server");
    loop {
        match warp::serve(filter.clone()).try_bind_ephemeral(server_addr) {
            Ok((addr, server_fut)) => {
                info!(%addr, "started HTTP server");
                return server_fut.await;
            }
            Err(error) => {
                if server_addr.port() == 0 {
                    warn!(%error, "failed to start HTTP server");
                    return;
                } else {
                    server_addr.set_port(0);
                    debug!(%error, "failed to start HTTP server. retrying on random port");
                }
            }
        }
    }
}

async fn parse_post_deploy_request<REv>(
    effect_builder: EffectBuilder<REv>,
    encoded_deploy: Bytes,
    ticket_counter: TicketCounterHandle,
) -> Result<WithStatus<Json>, Rejection>
where
    REv: From<Event> + From<ApiRequest> + Send,
{
    // If we're exceeding the acceptable rate of incoming deploys, return an HTTP 429.
    if !ticket_counter.acquire_ticket() {
        let error_reply = "Deploy rate limit exceeded.";
        let json = reply::json(&error_reply);
        return Ok(reply::with_status(json, StatusCode::TOO_MANY_REQUESTS));
    }

    let deploy = match str::from_utf8(encoded_deploy.as_ref())
        .map_err(|error| error.to_string())
        .and_then(|encoded_deploy_str| {
            Deploy::from_json(encoded_deploy_str).map_err(|error| error.to_string())
        }) {
        Ok(deploy) => deploy,
        Err(error) => {
            info!("failed to put deploy: {}", error);
            let error_reply = format!("Failed to parse as JSON-encoded Deploy: {}", error);
            let json = reply::json(&error_reply);
            return Ok(reply::with_status(json, StatusCode::BAD_REQUEST));
        }
    };

    effect_builder
        .make_request(
            |responder| ApiRequest::SubmitDeploy {
                deploy: Box::new(deploy),
                responder,
            },
            QueueKind::Api,
        )
        .await;

    let json = reply::json(&"");
    Ok(reply::with_status(json, StatusCode::OK))
}

async fn parse_get_deploy_request<REv>(
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
    let deploy_hashes = effect_builder
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

    let hex_hashes = deploy_hashes
        .into_iter()
        .map(|deploy_hash| hex::encode(deploy_hash.inner()))
        .collect::<Vec<_>>();
    // TODO - paginate these?
    let (body, status) = match serde_json::to_string(&hex_hashes) {
        Ok(body) => (body, StatusCode::OK),
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

    let maybe_deploy = effect_builder
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

    let (body, status) = match maybe_deploy {
        Some(deploy) => match deploy.to_json() {
            Ok(deploy_as_json) => (deploy_as_json, StatusCode::OK),
            Err(error) => (error_body(&error), StatusCode::INTERNAL_SERVER_ERROR),
        },
        None => ("null".to_string(), StatusCode::OK),
    };

    Ok(Response::builder()
        .header("content-type", "application/json")
        .status(status)
        .body(body)
        .unwrap())
}

async fn handle_get_status<REv>(
    effect_builder: EffectBuilder<REv>,
) -> Result<Response<String>, Rejection>
where
    REv: From<Event> + From<ApiRequest> + Send,
{
    let status_feed_json = effect_builder
        .make_request(
            |responder| ApiRequest::GetStatus { responder },
            QueueKind::Api,
        )
        .await;

    let (body, status) = match status_feed_json {
        Some(body) => (body, StatusCode::OK),
        None => (
            String::from("status unavailable"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    };

    Ok(Response::builder()
        .header("content-type", "application/json")
        .status(status)
        .body(body)
        .unwrap())
}

impl<REv, R> Component<REv, R> for ApiServer
where
    REv: From<ApiServerAnnouncement>
        + From<NetworkInfoRequest<NodeId>>
        + From<LinearChainRequest<NodeId>>
        + From<ContractRuntimeRequest>
        + From<MetricsRequest>
        + From<StorageRequest<Storage>>
        + Send,
    R: Rng + CryptoRng + ?Sized,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::ApiRequest(ApiRequest::SubmitDeploy { deploy, responder }) => {
                let mut effects = effect_builder.announce_deploy_received(deploy).ignore();
                effects.extend(responder.respond(()).ignore());
                effects
            }
            Event::ApiRequest(ApiRequest::GetDeploy { hash, responder }) => effect_builder
                .get_deploys_from_storage(smallvec![hash])
                .event(move |mut result| Event::GetDeployResult {
                    hash,
                    result: Box::new(result.pop().expect("can only contain one result")),
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::ListDeploys { responder }) => effect_builder
                .list_deploys()
                .event(move |result| Event::ListDeploysResult {
                    result,
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::GetMetrics { responder }) => effect_builder
                .get_metrics()
                .event(move |text| Event::GetMetricsResult {
                    text,
                    main_responder: responder,
                }),
            Event::ApiRequest(ApiRequest::GetStatus { responder }) => async move {
                let (last_finalized_block, peers) = join!(
                    effect_builder.get_last_finalized_block(),
                    effect_builder.network_peers()
                );
                let status_feed = StatusFeed::new(last_finalized_block, peers);
                debug!("GetStatus --status_feed: {:?}", status_feed);
                let json = {
                    match serde_json::to_string(&status_feed) {
                        Ok(json) => json,
                        Err(error) => {
                            error!("GetStatus --error: {:?}", error);
                            serde_json::to_string(&StatusFeed::default())
                                .unwrap_or_else(|_| String::default())
                        }
                    }
                };
                responder.respond(Some(json)).await;
            }
            .ignore(),
            Event::GetDeployResult {
                hash: _,
                result,
                main_responder,
            } => main_responder.respond(*result).ignore(),
            Event::ListDeploysResult {
                result,
                main_responder,
            } => main_responder.respond(result).ignore(),
            Event::GetMetricsResult {
                text,
                main_responder,
            } => main_responder.respond(text).ignore(),
        }
    }
}
