//! Types and functions used by the http server to manage the event-stream.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use datasize::DataSize;
use futures::{future, Stream, StreamExt};
use http::StatusCode;
use hyper::Body;
#[cfg(test)]
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc,
};
use tokio_stream::wrappers::{
    errors::BroadcastStreamRecvError, BroadcastStream, UnboundedReceiverStream,
};
use tracing::{debug, error, info, warn};
use warp::{
    filters::BoxedFilter,
    path,
    reject::Rejection,
    reply::Response,
    sse::{self, Event as WarpServerSentEvent},
    Filter, Reply,
};

use casper_types::{EraId, ExecutionEffect, ExecutionResult, ProtocolVersion, PublicKey};

use crate::types::{
    BlockHash, Deploy, DeployHash, FinalitySignature, JsonBlock, TimeDiff, Timestamp,
};
#[cfg(test)]
use crate::{crypto::AsymmetricKeyExt, testing, testing::TestRng, types::Block};

/// The URL root path.
pub const SSE_API_ROOT_PATH: &str = "events";
/// The URL path part to subscribe to all events other than `DeployAccepted`s and
/// `FinalitySignature`s.
pub const SSE_API_MAIN_PATH: &str = "main";
/// The URL path part to subscribe to only `DeployAccepted` events.
pub const SSE_API_DEPLOYS_PATH: &str = "deploys";
/// The URL path part to subscribe to only `FinalitySignature` events.
pub const SSE_API_SIGNATURES_PATH: &str = "sigs";
/// The URL query string field name.
pub const QUERY_FIELD: &str = "start_from";

/// The filter associated with `/events/main` path.
const MAIN_FILTER: [EventFilter; 5] = [
    EventFilter::BlockAdded,
    EventFilter::DeployProcessed,
    EventFilter::DeployExpired,
    EventFilter::Fault,
    EventFilter::Step,
];
/// The filter associated with `/events/deploys` path.
const DEPLOYS_FILTER: [EventFilter; 1] = [EventFilter::DeployAccepted];
/// The filter associated with `/events/sigs` path.
const SIGNATURES_FILTER: [EventFilter; 1] = [EventFilter::FinalitySignature];

/// The "id" field of the events sent on the event stream to clients.
pub type Id = u32;

/// The "data" field of the events sent on the event stream to clients.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize, JsonSchema)]
pub enum SseData {
    /// The version of this node's API server.  This event will always be the first sent to a new
    /// client, and will have no associated event ID provided.
    #[data_size(skip)]
    ApiVersion(ProtocolVersion),
    /// The given block has been added to the linear chain and stored locally.
    BlockAdded {
        block_hash: BlockHash,
        block: Box<JsonBlock>,
    },
    /// The given deploy has been newly-accepted by this node.
    DeployAccepted {
        #[schemars(with = "Deploy", description = "a deploy")]
        // It's an Arc to not create multiple copies of the same deploy for multiple subscribers.
        deploy: Arc<Deploy>,
    },
    /// The given deploy has been executed, committed and forms part of the given block.
    DeployProcessed {
        deploy_hash: Box<DeployHash>,
        account: Box<PublicKey>,
        timestamp: Timestamp,
        ttl: TimeDiff,
        dependencies: Vec<DeployHash>,
        block_hash: Box<BlockHash>,
        #[data_size(skip)]
        execution_result: Box<ExecutionResult>,
    },
    /// The given deploy has expired.
    DeployExpired { deploy_hash: DeployHash },
    /// Generic representation of validator's fault in an era.
    Fault {
        era_id: EraId,
        public_key: PublicKey,
        timestamp: Timestamp,
    },
    /// New finality signature received.
    FinalitySignature(Box<FinalitySignature>),
    /// The execution effects produced by a `StepRequest`.
    Step {
        era_id: EraId,
        #[data_size(skip)]
        execution_effect: ExecutionEffect,
    },
    /// The node is about to shut down.
    Shutdown,
}

impl SseData {
    pub(super) fn should_include(&self, filter: &[EventFilter]) -> bool {
        match self {
            SseData::ApiVersion(_) | SseData::Shutdown => true,
            SseData::BlockAdded { .. } => filter.contains(&EventFilter::BlockAdded),
            SseData::DeployAccepted { .. } => filter.contains(&EventFilter::DeployAccepted),
            SseData::DeployProcessed { .. } => filter.contains(&EventFilter::DeployProcessed),
            SseData::DeployExpired { .. } => filter.contains(&EventFilter::DeployExpired),
            SseData::Fault { .. } => filter.contains(&EventFilter::Fault),
            SseData::FinalitySignature(_) => filter.contains(&EventFilter::FinalitySignature),
            SseData::Step { .. } => filter.contains(&EventFilter::Step),
        }
    }
}

#[cfg(test)]
impl SseData {
    /// Returns a random `SseData::ApiVersion`.
    pub(super) fn random_api_version(rng: &mut TestRng) -> Self {
        let protocol_version = ProtocolVersion::from_parts(
            rng.gen_range(0..10),
            rng.gen::<u8>() as u32,
            rng.gen::<u8>() as u32,
        );
        SseData::ApiVersion(protocol_version)
    }

    /// Returns a random `SseData::BlockAdded`.
    pub(super) fn random_block_added(rng: &mut TestRng) -> Self {
        let block = Block::random(rng);
        SseData::BlockAdded {
            block_hash: *block.hash(),
            block: Box::new(JsonBlock::new(block, None)),
        }
    }

    /// Returns a random `SseData::DeployAccepted`, along with the random `Deploy`.
    pub(super) fn random_deploy_accepted(rng: &mut TestRng) -> (Self, Deploy) {
        let deploy = Deploy::random(rng);
        let event = SseData::DeployAccepted {
            deploy: Arc::new(deploy.clone()),
        };
        (event, deploy)
    }

    /// Returns a random `SseData::DeployProcessed`.
    pub(super) fn random_deploy_processed(rng: &mut TestRng) -> Self {
        let deploy = Deploy::random(rng);
        SseData::DeployProcessed {
            deploy_hash: Box::new(*deploy.id()),
            account: Box::new(deploy.header().account().clone()),
            timestamp: deploy.header().timestamp(),
            ttl: deploy.header().ttl(),
            dependencies: deploy.header().dependencies().clone(),
            block_hash: Box::new(BlockHash::random(rng)),
            execution_result: Box::new(rng.gen()),
        }
    }

    /// Returns a random `SseData::DeployExpired`
    pub(super) fn random_deploy_expired(rng: &mut TestRng) -> Self {
        let deploy = testing::create_expired_deploy(Timestamp::now(), rng);
        SseData::DeployExpired {
            deploy_hash: *deploy.id(),
        }
    }

    /// Returns a random `SseData::Fault`.
    pub(super) fn random_fault(rng: &mut TestRng) -> Self {
        SseData::Fault {
            era_id: EraId::new(rng.gen()),
            public_key: PublicKey::random(rng),
            timestamp: Timestamp::random(rng),
        }
    }

    /// Returns a random `SseData::FinalitySignature`.
    pub(super) fn random_finality_signature(rng: &mut TestRng) -> Self {
        SseData::FinalitySignature(Box::new(FinalitySignature::random_for_block(
            BlockHash::random(rng),
            rng.gen(),
        )))
    }

    /// Returns a random `SseData::Step`.
    pub(super) fn random_step(rng: &mut TestRng) -> Self {
        let execution_effect = match rng.gen::<ExecutionResult>() {
            ExecutionResult::Success { effect, .. } | ExecutionResult::Failure { effect, .. } => {
                effect
            }
        };
        SseData::Step {
            era_id: EraId::new(rng.gen()),
            execution_effect,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct DeployAccepted {
    pub(super) deploy_accepted: Arc<Deploy>,
}

/// The components of a single SSE.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct ServerSentEvent {
    /// The ID should only be `None` where the `data` is `SseData::ApiVersion`.
    pub(super) id: Option<Id>,
    pub(super) data: SseData,
}

impl ServerSentEvent {
    /// The first event sent to every subscribing client.
    pub(super) fn initial_event(client_api_version: ProtocolVersion) -> Self {
        ServerSentEvent {
            id: None,
            data: SseData::ApiVersion(client_api_version),
        }
    }
}

/// The messages sent via the tokio broadcast channel to the handler of each client's SSE stream.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(clippy::large_enum_variant)]
pub(super) enum BroadcastChannelMessage {
    /// The message should be sent to the client as an SSE with an optional ID.  The ID should only
    /// be `None` where the `data` is `SseData::ApiVersion`.
    ServerSentEvent(ServerSentEvent),
    /// The stream should terminate as the server is shutting down.
    ///
    /// Note: ideally, we'd just drop all the tokio broadcast channel senders to make the streams
    /// terminate naturally, but we can't drop the sender cloned into warp filter.
    Shutdown,
}

/// Passed to the server whenever a new client subscribes.
pub(super) struct NewSubscriberInfo {
    /// The event ID from which the stream should start for this client.
    pub(super) start_from: Option<Id>,
    /// A channel to send the initial events to the client's handler.  This will always send the
    /// ApiVersion as the first event, and then any buffered events as indicated by `start_from`.
    pub(super) initial_events_sender: mpsc::UnboundedSender<ServerSentEvent>,
}

/// A filter for event types a client has subscribed to receive.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub(super) enum EventFilter {
    BlockAdded,
    DeployAccepted,
    DeployProcessed,
    DeployExpired,
    Fault,
    FinalitySignature,
    Step,
}

/// Filters the `event`, mapping it to a warp event, or `None` if it should be filtered out.
async fn filter_map_server_sent_event(
    event: &ServerSentEvent,
    event_filter: &[EventFilter],
) -> Option<Result<WarpServerSentEvent, RecvError>> {
    if !event.data.should_include(event_filter) {
        return None;
    }

    let id = match event.id {
        Some(id) => {
            if matches!(&event.data, &SseData::ApiVersion { .. }) {
                error!("ApiVersion should have no event ID");
                return None;
            }
            id.to_string()
        }
        None => {
            if !matches!(&event.data, &SseData::ApiVersion { .. }) {
                error!("only ApiVersion may have no event ID");
                return None;
            }
            String::new()
        }
    };

    match &event.data {
        &SseData::ApiVersion { .. } => Some(Ok(WarpServerSentEvent::default()
            .json_data(&event.data)
            .unwrap_or_else(|error| {
                warn!(%error, ?event, "failed to jsonify sse event");
                WarpServerSentEvent::default()
            }))),

        &SseData::BlockAdded { .. }
        | &SseData::DeployProcessed { .. }
        | &SseData::DeployExpired { .. }
        | &SseData::Fault { .. }
        | &SseData::Step { .. }
        | &SseData::FinalitySignature(_)
        | &SseData::Shutdown => Some(Ok(WarpServerSentEvent::default()
            .json_data(&event.data)
            .unwrap_or_else(|error| {
                warn!(%error, ?event, "failed to jsonify sse event");
                WarpServerSentEvent::default()
            })
            .id(id))),

        SseData::DeployAccepted { deploy } => Some(Ok(WarpServerSentEvent::default()
            .json_data(&DeployAccepted {
                deploy_accepted: deploy.clone(),
            })
            .unwrap_or_else(|error| {
                warn!(%error, "failed to jsonify sse event");
                WarpServerSentEvent::default()
            })
            .id(event.id.unwrap().to_string()))),
    }
}

/// Converts the final URL path element to a slice of `EventFilter`s.
pub(super) fn get_filter(path_param: &str) -> Option<&'static [EventFilter]> {
    match path_param {
        SSE_API_MAIN_PATH => Some(&MAIN_FILTER[..]),
        SSE_API_DEPLOYS_PATH => Some(&DEPLOYS_FILTER[..]),
        SSE_API_SIGNATURES_PATH => Some(&SIGNATURES_FILTER[..]),
        _ => None,
    }
}

/// Extracts the starting event ID from the provided query, or `None` if `query` is empty.
///
/// If `query` is not empty, returns a 422 response if `query` doesn't have exactly one entry,
/// "starts_from" mapped to a value representing an event ID.
fn parse_query(query: HashMap<String, String>) -> Result<Option<Id>, Response> {
    if query.is_empty() {
        return Ok(None);
    }

    if query.len() > 1 {
        return Err(create_422());
    }

    match query
        .get(QUERY_FIELD)
        .and_then(|id_str| id_str.parse::<Id>().ok())
    {
        Some(id) => Ok(Some(id)),
        None => Err(create_422()),
    }
}

/// Creates a 404 response with a useful error message in the body.
fn create_404() -> Response {
    let mut response = Response::new(Body::from(format!(
        "invalid path: expected '/{root}/{main}', '/{root}/{deploys}' or '/{root}/{sigs}'\n",
        root = SSE_API_ROOT_PATH,
        main = SSE_API_MAIN_PATH,
        deploys = SSE_API_DEPLOYS_PATH,
        sigs = SSE_API_SIGNATURES_PATH
    )));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

/// Creates a 422 response with a useful error message in the body for use in case of a bad query
/// string.
fn create_422() -> Response {
    let mut response = Response::new(Body::from(format!(
        "invalid query: expected single field '{}=<EVENT ID>'\n",
        QUERY_FIELD
    )));
    *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
    response
}

/// Creates a 503 response (Service Unavailable) to be returned if the server has too many
/// subscribers.
fn create_503() -> Response {
    let mut response = Response::new(Body::from("server has reached limit of subscribers"));
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    response
}

pub(super) struct ChannelsAndFilter {
    pub(super) event_broadcaster: broadcast::Sender<BroadcastChannelMessage>,
    pub(super) new_subscriber_info_receiver: mpsc::UnboundedReceiver<NewSubscriberInfo>,
    pub(super) sse_filter: BoxedFilter<(Response,)>,
}

impl ChannelsAndFilter {
    /// Creates the message-passing channels required to run the event-stream server and the warp
    /// filter for the event-stream server.
    pub(super) fn new(broadcast_channel_size: usize, max_concurrent_subscribers: u32) -> Self {
        // Create a channel to broadcast new events to all subscribed clients' streams.
        let (event_broadcaster, _) = broadcast::channel(broadcast_channel_size);
        let cloned_broadcaster = event_broadcaster.clone();

        // Create a channel for `NewSubscriberInfo`s to pass the information required to handle a
        // new client subscription.
        let (new_subscriber_info_sender, new_subscriber_info_receiver) = mpsc::unbounded_channel();

        let sse_filter = warp::get()
            .and(path(SSE_API_ROOT_PATH))
            .and(path::param::<String>())
            .and(path::end())
            .and(warp::query())
            .map(move |path_param: String, query: HashMap<String, String>| {
                // If we already have the maximum number of subscribers, reject this new one.
                if cloned_broadcaster.receiver_count() >= max_concurrent_subscribers as usize {
                    info!(
                        %max_concurrent_subscribers,
                        "event stream server has max subscribers: rejecting new one"
                    );
                    return create_503();
                }

                // If `path_param` is not a valid string, return a 404.
                let event_filter = match get_filter(path_param.as_str()) {
                    Some(filter) => filter,
                    None => return create_404(),
                };

                let start_from = match parse_query(query) {
                    Ok(maybe_id) => maybe_id,
                    Err(error_response) => return error_response,
                };

                // Create a channel for the client's handler to receive the stream of initial
                // events.
                let (initial_events_sender, initial_events_receiver) = mpsc::unbounded_channel();

                // Supply the server with the sender part of the channel along with the client's
                // requested starting point.
                let new_subscriber_info = NewSubscriberInfo {
                    start_from,
                    initial_events_sender,
                };
                if new_subscriber_info_sender
                    .send(new_subscriber_info)
                    .is_err()
                {
                    error!("failed to send new subscriber info");
                }

                // Create a channel for the client's handler to receive the stream of ongoing
                // events.
                let ongoing_events_receiver = cloned_broadcaster.subscribe();

                sse::reply(sse::keep_alive().stream(stream_to_client(
                    initial_events_receiver,
                    ongoing_events_receiver,
                    event_filter,
                )))
                .into_response()
            })
            .or_else(|_| async move { Ok::<_, Rejection>((create_404(),)) })
            .boxed();

        ChannelsAndFilter {
            event_broadcaster,
            new_subscriber_info_receiver,
            sse_filter,
        }
    }
}

/// This takes the two channel receivers and turns them into a stream of SSEs to the subscribed
/// client.
///
/// The initial events receiver (an mpsc receiver) is exhausted first, and contains an initial
/// `ApiVersion` message, followed by any historical events the client requested using the query
/// string.
///
/// The ongoing events channel (a broadcast receiver) is then consumed, and will remain in use until
/// either the client disconnects, or the server shuts down (indicated by sending a `Shutdown`
/// variant via the channel).  This channel will receive all SSEs created from the moment the client
/// subscribed to the server's event stream.
///
/// It also takes an `EventFilter` which causes events to which the client didn't subscribe to be
/// skipped.
fn stream_to_client(
    initial_events: mpsc::UnboundedReceiver<ServerSentEvent>,
    ongoing_events: broadcast::Receiver<BroadcastChannelMessage>,
    event_filter: &'static [EventFilter],
) -> impl Stream<Item = Result<WarpServerSentEvent, RecvError>> + 'static {
    // Keep a record of the IDs of the events delivered via the `initial_events` receiver.
    let initial_stream_ids = Arc::new(RwLock::new(HashSet::new()));
    let cloned_initial_ids = Arc::clone(&initial_stream_ids);

    // Map the events arriving after the initial stream to the correct error type, filtering out any
    // that have already been sent in the initial stream.
    let ongoing_stream = BroadcastStream::new(ongoing_events)
        .filter_map(move |result| {
            let cloned_initial_ids = Arc::clone(&cloned_initial_ids);
            async move {
                match result {
                    Ok(BroadcastChannelMessage::ServerSentEvent(event)) => {
                        if let Some(id) = event.id {
                            if cloned_initial_ids.read().unwrap().contains(&id) {
                                debug!(event_id=%id, "skipped duplicate event");
                                return None;
                            }
                        }
                        Some(Ok(event))
                    }
                    Ok(BroadcastChannelMessage::Shutdown) => Some(Err(RecvError::Closed)),
                    Err(BroadcastStreamRecvError::Lagged(amount)) => {
                        info!(
                            "client lagged by {} events - dropping event stream connection to client",
                            amount
                        );
                        Some(Err(RecvError::Lagged(amount)))
                    }
                }
            }
        })
        .take_while(|result| future::ready(!matches!(result, Err(RecvError::Closed))));

    // Serve the initial events followed by the ongoing ones, filtering as dictated by the
    // `event_filter`.
    UnboundedReceiverStream::new(initial_events)
        .map(move |event| {
            if let Some(id) = event.id {
                let _ = initial_stream_ids.write().unwrap().insert(id);
            }
            Ok(event)
        })
        .chain(ongoing_stream)
        .filter_map(move |result| async move {
            match result {
                Ok(event) => filter_map_server_sent_event(&event, event_filter).await,
                Err(error) => Some(Err(error)),
            }
        })
}

#[cfg(test)]
mod tests {
    use std::iter;

    use super::*;
    use crate::{logging, testing::TestRng};

    async fn should_filter_out(event: &ServerSentEvent, filter: &'static [EventFilter]) {
        assert!(
            filter_map_server_sent_event(event, filter).await.is_none(),
            "should filter out {:?} with {:?}",
            event,
            filter
        );
    }

    async fn should_not_filter_out(event: &ServerSentEvent, filter: &'static [EventFilter]) {
        assert!(
            filter_map_server_sent_event(event, filter).await.is_some(),
            "should not filter out {:?} with {:?}",
            event,
            filter
        );
    }

    /// This test checks that events with correct IDs (i.e. all types have an ID except for
    /// `ApiVersion`) are filtered properly.
    #[tokio::test]
    async fn should_filter_events_with_valid_ids() {
        let _ = logging::init();
        let mut rng = crate::new_rng();

        let api_version = ServerSentEvent {
            id: None,
            data: SseData::random_api_version(&mut rng),
        };
        let block_added = ServerSentEvent {
            id: Some(rng.gen()),
            data: SseData::random_block_added(&mut rng),
        };
        let (sse_data, deploy) = SseData::random_deploy_accepted(&mut rng);
        let deploy_accepted = ServerSentEvent {
            id: Some(rng.gen()),
            data: sse_data,
        };
        let mut deploys = HashMap::new();
        let _ = deploys.insert(*deploy.id(), deploy);
        let deploy_processed = ServerSentEvent {
            id: Some(rng.gen()),
            data: SseData::random_deploy_processed(&mut rng),
        };
        let deploy_expired = ServerSentEvent {
            id: Some(rng.gen()),
            data: SseData::random_deploy_expired(&mut rng),
        };
        let fault = ServerSentEvent {
            id: Some(rng.gen()),
            data: SseData::random_fault(&mut rng),
        };
        let finality_signature = ServerSentEvent {
            id: Some(rng.gen()),
            data: SseData::random_finality_signature(&mut rng),
        };
        let step = ServerSentEvent {
            id: Some(rng.gen()),
            data: SseData::random_step(&mut rng),
        };
        let shutdown = ServerSentEvent {
            id: Some(rng.gen()),
            data: SseData::Shutdown,
        };

        // `EventFilter::Main` should only filter out `DeployAccepted`s and `FinalitySignature`s.
        should_not_filter_out(&api_version, &MAIN_FILTER[..]).await;
        should_not_filter_out(&block_added, &MAIN_FILTER[..]).await;
        should_not_filter_out(&deploy_processed, &MAIN_FILTER[..]).await;
        should_not_filter_out(&deploy_expired, &MAIN_FILTER[..]).await;
        should_not_filter_out(&fault, &MAIN_FILTER[..]).await;
        should_not_filter_out(&step, &MAIN_FILTER[..]).await;
        should_not_filter_out(&shutdown, &MAIN_FILTER).await;

        should_filter_out(&deploy_accepted, &MAIN_FILTER[..]).await;
        should_filter_out(&finality_signature, &MAIN_FILTER[..]).await;

        // `EventFilter::DeployAccepted` should filter out everything except `ApiVersion`s and
        // `DeployAccepted`s.
        should_not_filter_out(&api_version, &DEPLOYS_FILTER[..]).await;
        should_not_filter_out(&deploy_accepted, &DEPLOYS_FILTER[..]).await;
        should_not_filter_out(&shutdown, &DEPLOYS_FILTER[..]).await;

        should_filter_out(&block_added, &DEPLOYS_FILTER[..]).await;
        should_filter_out(&deploy_processed, &DEPLOYS_FILTER[..]).await;
        should_filter_out(&deploy_expired, &DEPLOYS_FILTER[..]).await;
        should_filter_out(&fault, &DEPLOYS_FILTER[..]).await;
        should_filter_out(&finality_signature, &DEPLOYS_FILTER[..]).await;
        should_filter_out(&step, &DEPLOYS_FILTER[..]).await;

        // `EventFilter::Signatures` should filter out everything except `ApiVersion`s and
        // `FinalitySignature`s.
        should_not_filter_out(&api_version, &SIGNATURES_FILTER[..]).await;
        should_not_filter_out(&finality_signature, &SIGNATURES_FILTER[..]).await;
        should_not_filter_out(&shutdown, &SIGNATURES_FILTER[..]).await;

        should_filter_out(&block_added, &SIGNATURES_FILTER[..]).await;
        should_filter_out(&deploy_accepted, &SIGNATURES_FILTER[..]).await;
        should_filter_out(&deploy_processed, &SIGNATURES_FILTER[..]).await;
        should_filter_out(&deploy_expired, &SIGNATURES_FILTER[..]).await;
        should_filter_out(&fault, &SIGNATURES_FILTER[..]).await;
        should_filter_out(&step, &SIGNATURES_FILTER[..]).await;
    }

    /// This test checks that events with incorrect IDs (i.e. no types have an ID except for
    /// `ApiVersion`) are filtered out.
    #[tokio::test]
    async fn should_filter_events_with_invalid_ids() {
        let _ = logging::init();
        let mut rng = crate::new_rng();

        let malformed_api_version = ServerSentEvent {
            id: Some(rng.gen()),
            data: SseData::random_api_version(&mut rng),
        };
        let malformed_block_added = ServerSentEvent {
            id: None,
            data: SseData::random_block_added(&mut rng),
        };
        let (sse_data, deploy) = SseData::random_deploy_accepted(&mut rng);
        let malformed_deploy_accepted = ServerSentEvent {
            id: None,
            data: sse_data,
        };
        let mut deploys = HashMap::new();
        let _ = deploys.insert(*deploy.id(), deploy);
        let malformed_deploy_processed = ServerSentEvent {
            id: None,
            data: SseData::random_deploy_processed(&mut rng),
        };
        let malformed_deploy_expired = ServerSentEvent {
            id: None,
            data: SseData::random_deploy_expired(&mut rng),
        };
        let malformed_fault = ServerSentEvent {
            id: None,
            data: SseData::random_fault(&mut rng),
        };
        let malformed_finality_signature = ServerSentEvent {
            id: None,
            data: SseData::random_finality_signature(&mut rng),
        };
        let malformed_step = ServerSentEvent {
            id: None,
            data: SseData::random_step(&mut rng),
        };
        let malformed_shutdown = ServerSentEvent {
            id: None,
            data: SseData::Shutdown,
        };

        for filter in &[
            &MAIN_FILTER[..],
            &DEPLOYS_FILTER[..],
            &SIGNATURES_FILTER[..],
        ] {
            should_filter_out(&malformed_api_version, filter).await;
            should_filter_out(&malformed_block_added, filter).await;
            should_filter_out(&malformed_deploy_accepted, filter).await;
            should_filter_out(&malformed_deploy_processed, filter).await;
            should_filter_out(&malformed_deploy_expired, filter).await;
            should_filter_out(&malformed_fault, filter).await;
            should_filter_out(&malformed_finality_signature, filter).await;
            should_filter_out(&malformed_step, filter).await;
            should_filter_out(&malformed_shutdown, filter).await;
        }
    }

    async fn should_filter_duplicate_events(path_filter: &str) {
        // Returns `count` random SSE events, all of a single variant defined by `path_filter`.  The
        // events will have sequential IDs starting from `start_id`, and if the path filter
        // indicates the events should be deploy-accepted ones, the corresponding random deploys
        // will be inserted into `deploys`.
        fn make_random_events(
            rng: &mut TestRng,
            start_id: Id,
            count: usize,
            path_filter: &str,
            deploys: &mut HashMap<DeployHash, Deploy>,
        ) -> Vec<ServerSentEvent> {
            (start_id..(start_id + count as u32))
                .map(|id| {
                    let data = match path_filter {
                        SSE_API_MAIN_PATH => SseData::random_block_added(rng),
                        SSE_API_DEPLOYS_PATH => {
                            let (event, deploy) = SseData::random_deploy_accepted(rng);
                            assert!(deploys.insert(*deploy.id(), deploy).is_none());
                            event
                        }
                        SSE_API_SIGNATURES_PATH => SseData::random_finality_signature(rng),
                        _ => unreachable!(),
                    };
                    ServerSentEvent { id: Some(id), data }
                })
                .collect()
        }

        // Returns `NUM_ONGOING_EVENTS` random SSE events for the ongoing stream containing
        // duplicates taken from the end of the initial stream.  Allows for the full initial stream
        // to be duplicated except for its first event (the `ApiVersion` one) which has no ID.
        fn make_ongoing_events(
            rng: &mut TestRng,
            duplicate_count: usize,
            initial_events: &[ServerSentEvent],
            path_filter: &str,
            deploys: &mut HashMap<DeployHash, Deploy>,
        ) -> Vec<ServerSentEvent> {
            assert!(duplicate_count < initial_events.len());
            let initial_skip_count = initial_events.len() - duplicate_count;
            let unique_start_id = initial_events.len() as Id - 1;
            let unique_count = NUM_ONGOING_EVENTS - duplicate_count;
            initial_events
                .iter()
                .skip(initial_skip_count)
                .cloned()
                .chain(make_random_events(
                    rng,
                    unique_start_id,
                    unique_count,
                    path_filter,
                    deploys,
                ))
                .collect()
        }

        // The number of events in the initial stream, excluding the very first `ApiVersion` one.
        const NUM_INITIAL_EVENTS: usize = 10;
        // The number of events in the ongoing stream, including any duplicated from the initial
        // stream.
        const NUM_ONGOING_EVENTS: usize = 20;

        let _ = logging::init();
        let mut rng = crate::new_rng();

        let mut deploys = HashMap::new();

        let initial_events: Vec<ServerSentEvent> =
            iter::once(ServerSentEvent::initial_event(ProtocolVersion::V1_0_0))
                .chain(make_random_events(
                    &mut rng,
                    0,
                    NUM_INITIAL_EVENTS,
                    path_filter,
                    &mut deploys,
                ))
                .collect();

        // Run three cases; where only a single event is duplicated, where five are duplicated, and
        // where the whole initial stream (except the `ApiVersion`) is duplicated.
        for duplicate_count in &[1, 5, NUM_INITIAL_EVENTS] {
            // Create the events with the requisite duplicates at the start of the collection.
            let ongoing_events = make_ongoing_events(
                &mut rng,
                *duplicate_count,
                &initial_events,
                path_filter,
                &mut deploys,
            );

            let (initial_events_sender, initial_events_receiver) = mpsc::unbounded_channel();
            let (ongoing_events_sender, ongoing_events_receiver) =
                broadcast::channel(NUM_INITIAL_EVENTS + NUM_ONGOING_EVENTS + 1);

            // Send all the events.
            for event in initial_events.iter().cloned() {
                initial_events_sender.send(event).unwrap();
            }
            for event in ongoing_events.iter().cloned() {
                let _ = ongoing_events_sender
                    .send(BroadcastChannelMessage::ServerSentEvent(event))
                    .unwrap();
            }
            // Drop the channel senders so that the chained receiver streams can both complete.
            drop(initial_events_sender);
            drop(ongoing_events_sender);

            // Collect the events emitted by `stream_to_client()` - should not contain duplicates.
            let received_events: Vec<Result<WarpServerSentEvent, RecvError>> = stream_to_client(
                initial_events_receiver,
                ongoing_events_receiver,
                get_filter(path_filter).unwrap(),
            )
            .collect()
            .await;

            // Create the expected collection of emitted events.
            let deduplicated_events: Vec<ServerSentEvent> = initial_events
                .iter()
                .take(initial_events.len() - duplicate_count)
                .cloned()
                .chain(ongoing_events)
                .collect();

            assert_eq!(received_events.len(), deduplicated_events.len());

            // Iterate the received and expected collections, asserting that each matches.  As we
            // don't have access to the internals of the `WarpServerSentEvent`s, assert using their
            // `String` representations.
            for (received_event, deduplicated_event) in
                received_events.iter().zip(deduplicated_events.iter())
            {
                let received_event = received_event.as_ref().unwrap();

                let expected_data_string = match &deduplicated_event.data {
                    SseData::DeployAccepted { deploy } => serde_json::to_string(&DeployAccepted {
                        deploy_accepted: deploy.clone(),
                    })
                    .unwrap(),
                    data => serde_json::to_string(&data).unwrap(),
                };

                let expected_id_string = if let Some(id) = deduplicated_event.id {
                    format!("\nid:{}", id)
                } else {
                    String::new()
                };

                let expected_string =
                    format!("data:{}{}", expected_data_string, expected_id_string);

                assert_eq!(received_event.to_string().trim(), expected_string)
            }
        }
    }

    /// This test checks that main events from the initial stream which are duplicated in the
    /// ongoing stream are filtered out.
    #[tokio::test]
    async fn should_filter_duplicate_main_events() {
        should_filter_duplicate_events(SSE_API_MAIN_PATH).await
    }

    /// This test checks that deploy-accepted events from the initial stream which are duplicated in
    /// the ongoing stream are filtered out.
    #[tokio::test]
    async fn should_filter_duplicate_deploys_events() {
        should_filter_duplicate_events(SSE_API_DEPLOYS_PATH).await
    }

    /// This test checks that signature events from the initial stream which are duplicated in the
    /// ongoing stream are filtered out.
    #[tokio::test]
    async fn should_filter_duplicate_signature_events() {
        should_filter_duplicate_events(SSE_API_SIGNATURES_PATH).await
    }
}
