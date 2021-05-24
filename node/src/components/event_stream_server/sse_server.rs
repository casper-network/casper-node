//! Types and functions used by the http server to manage the event-stream.

use datasize::DataSize;
use futures::{Stream, StreamExt};
use http::status::StatusCode;
use hyper::Body;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc,
};
use tokio_stream::wrappers::{
    errors::BroadcastStreamRecvError, BroadcastStream, UnboundedReceiverStream,
};
use tracing::{error, info, trace, warn};
use warp::{
    filters::BoxedFilter,
    path,
    reject::Rejection,
    reply::Response,
    sse::{self, Event as WarpServerSentEvent},
    Filter, Reply,
};

use casper_types::{EraId, ExecutionEffect, ExecutionResult, ProtocolVersion, PublicKey};

use crate::types::{BlockHash, DeployHash, FinalitySignature, JsonBlock, TimeDiff, Timestamp};
#[cfg(test)]
use crate::{
    crypto::AsymmetricKeyExt,
    testing::TestRng,
    types::{Block, Deploy},
};

/// The URL root path.
pub const SSE_API_ROOT_PATH: &str = "events";
/// The URL path part to subscribe to all events other than `FinalitySignature`s.
pub const SSE_API_MAIN_PATH: &str = "main";
/// The URL path part to subscribe to only `FinalitySignature` events.
pub const SSE_API_SIGNATURES_PATH: &str = "sigs";

/// The "id" field of the events sent on the event stream to clients.
type Id = u32;

/// The "data" field of the events sent on the event stream to clients.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
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
    /// Generic representation of validator's fault in an era.
    Fault {
        era_id: EraId,
        public_key: PublicKey,
        timestamp: Timestamp,
    },
    /// New finality signature received.
    FinalitySignature(Box<FinalitySignature>),
    Step {
        era_id: EraId,
        #[data_size(skip)]
        execution_effect: ExecutionEffect,
    },
}

#[cfg(test)]
impl SseData {
    /// Returns a random `SseData::ApiVersion`.
    fn random_api_version(rng: &mut TestRng) -> Self {
        let protocol_version = ProtocolVersion::from_parts(
            rng.gen_range(0..10),
            rng.gen::<u8>() as u32,
            rng.gen::<u8>() as u32,
        );
        SseData::ApiVersion(protocol_version)
    }

    /// Returns a random `SseData::BlockAdded`.
    fn random_block_added(rng: &mut TestRng) -> Self {
        let block = Block::random(rng);
        SseData::BlockAdded {
            block_hash: *block.hash(),
            block: Box::new(JsonBlock::new(block, None)),
        }
    }

    /// Returns a random `SseData::DeployProcessed`.
    fn random_deploy_processed(rng: &mut TestRng) -> Self {
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

    /// Returns a random `SseData::Fault`.
    fn random_fault(rng: &mut TestRng) -> Self {
        SseData::Fault {
            era_id: EraId::new(rng.gen()),
            public_key: PublicKey::random(rng),
            timestamp: Timestamp::random(rng),
        }
    }

    /// Returns a random `SseData::FinalitySignature`.
    fn random_finality_signature(rng: &mut TestRng) -> Self {
        SseData::FinalitySignature(Box::new(FinalitySignature::random_for_block(
            BlockHash::random(rng),
            rng.gen(),
        )))
    }

    /// Returns a random `SseData::Step`.
    fn random_step(rng: &mut TestRng) -> Self {
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

impl From<ServerSentEvent> for WarpServerSentEvent {
    fn from(event: ServerSentEvent) -> Self {
        match (event.id, &event.data) {
            (id, &SseData::ApiVersion { .. }) => {
                if id.is_some() {
                    error!("ApiVersion should have no event ID");
                }
                WarpServerSentEvent::default()
                    .json_data(&event.data)
                    .unwrap_or_default()
            }

            (Some(id), &SseData::BlockAdded { .. })
            | (Some(id), &SseData::DeployProcessed { .. })
            | (Some(id), &SseData::Fault { .. })
            | (Some(id), &SseData::Step { .. })
            | (Some(id), &SseData::FinalitySignature(_)) => WarpServerSentEvent::default()
                .json_data(&event.data)
                .unwrap_or_else(|error| {
                    warn!(%error, ?event, "failed to jsonify sse event");
                    WarpServerSentEvent::default()
                })
                .id(id.to_string()),

            (None, &SseData::BlockAdded { .. })
            | (None, &SseData::DeployProcessed { .. })
            | (None, &SseData::Fault { .. })
            | (None, &SseData::Step { .. })
            | (None, &SseData::FinalitySignature(_)) => {
                error!("only ApiVersion may have no event ID");
                WarpServerSentEvent::default()
            }
        }
    }
}

/// The messages sent via the tokio broadcast channel to the handler of each client's SSE stream.
#[derive(Clone, PartialEq, Eq, Debug)]
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

/// The endpoint's query string, e.g. `http://localhost:22777/events?start_from=999`
#[derive(Deserialize, Debug)]
struct Query {
    start_from: Option<Id>,
}

/// A filter for event types a client has subscribed to receive.
#[derive(Clone, Copy, Eq, PartialEq)]
enum EventFilter {
    /// All events other than `FinalitySignature`s.
    Main,
    /// The initial `ApiVersion` event and then only `FinalitySignature` events.
    Signatures,
}

impl EventFilter {
    fn new(filter: &str) -> Option<Self> {
        match filter {
            SSE_API_MAIN_PATH => Some(EventFilter::Main),
            SSE_API_SIGNATURES_PATH => Some(EventFilter::Signatures),
            _ => None,
        }
    }
}

/// Filters the `event`, mapping it to a warp event, or `None` if it should be filtered out.
fn filter_map_server_sent_event(
    event: ServerSentEvent,
    event_filter: EventFilter,
) -> Option<Result<WarpServerSentEvent, RecvError>> {
    match (&event.data, event_filter) {
        (&SseData::ApiVersion { .. }, _) // ApiVersion doesn't ever get filtered out.
        | (&SseData::BlockAdded { .. }, EventFilter::Main)
        | (&SseData::DeployProcessed { .. }, EventFilter::Main)
        | (&SseData::Fault { .. }, EventFilter::Main)
        | (&SseData::Step { .. }, EventFilter::Main)
        | (&SseData::FinalitySignature(_), EventFilter::Signatures) => Some(Ok(event.into())),

        (&SseData::BlockAdded { .. }, _)
        | (&SseData::DeployProcessed { .. }, _)
        | (&SseData::Fault { .. }, _)
        | (&SseData::Step { .. }, _)
        | (&SseData::FinalitySignature(_), _) => None,
    }
}

/// Creates a 404 response with a useful error message in the body.
fn create_404() -> Response {
    let mut response = Response::new(Body::from(format!(
        "invalid path: expected '{root}/{main}' or '{root}/{sigs}'\n",
        root = SSE_API_ROOT_PATH,
        main = SSE_API_MAIN_PATH,
        sigs = SSE_API_SIGNATURES_PATH
    )));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

/// Creates the message-passing channels required to run the event-stream server and the warp filter
/// for the event-stream server.
pub(super) fn create_channels_and_filter(
    broadcast_channel_size: usize,
) -> (
    broadcast::Sender<BroadcastChannelMessage>,
    mpsc::UnboundedReceiver<NewSubscriberInfo>,
    BoxedFilter<(Response,)>,
) {
    // Create a channel to broadcast new events to all subscribed clients' streams.
    let (broadcaster, _) = broadcast::channel(broadcast_channel_size);
    let cloned_broadcaster = broadcaster.clone();

    // Create a channel for `NewSubscriberInfo`s to pass the information required to handle a new
    // client subscription.
    let (new_subscriber_info_sender, new_subscriber_info_receiver) = mpsc::unbounded_channel();

    let filter = warp::get()
        .and(path(SSE_API_ROOT_PATH))
        .and(path::param::<String>())
        .and(path::end())
        .and(warp::query())
        .map(move |path_param: String, query: Query| {
            // If `path_param` is not a valid string, return a 404.
            let event_filter = match EventFilter::new(&path_param) {
                Some(filter) => filter,
                None => return create_404(),
            };

            // Create a channel for the client's handler to receive the stream of initial events.
            let (initial_events_sender, initial_events_receiver) = mpsc::unbounded_channel();

            // Supply the server with the sender part of the channel along with the client's
            // requested starting point.
            let new_subscriber_info = NewSubscriberInfo {
                start_from: query.start_from,
                initial_events_sender,
            };
            if new_subscriber_info_sender
                .send(new_subscriber_info)
                .is_err()
            {
                error!("failed to send new subscriber info");
            }

            // Create a channel for the client's handler to receive the stream of ongoing events.
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

    (broadcaster, new_subscriber_info_receiver, filter)
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
    event_filter: EventFilter,
) -> impl Stream<Item = Result<WarpServerSentEvent, RecvError>> + 'static {
    UnboundedReceiverStream::new(initial_events)
        .map(|event| Ok(BroadcastChannelMessage::ServerSentEvent(event)))
        .chain(BroadcastStream::new(ongoing_events))
        .filter_map(move |result| async move {
            trace!(?result);
            match result {
                Ok(BroadcastChannelMessage::ServerSentEvent(event)) => {
                    filter_map_server_sent_event(event, event_filter)
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
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging;

    // Checks the given event is correctly converted to a warp server sent event.  Since the warp
    // type is opaque to us, we just check via its `Display` impl.
    fn assert_event_converts(original_event: ServerSentEvent) {
        // Even if an ID is provided with an `ApiVersion` variant, it should be ignored.
        let expected_id_output = match original_event.data {
            SseData::ApiVersion(_) => String::new(),
            _ => format!("id:{}", original_event.id.unwrap()),
        };
        let expected_data_output = format!(
            "data:{}",
            serde_json::to_string(&original_event.data).unwrap()
        );

        let converted_event = WarpServerSentEvent::from(original_event);
        let actual_output = converted_event.to_string();

        assert!(
            actual_output.contains(&expected_data_output),
            "actual output:   {}\nexpected output: {}\n",
            actual_output,
            expected_data_output
        );

        if expected_id_output.is_empty() {
            assert!(
                !actual_output.contains("id:"),
                "actual output: {}",
                actual_output
            );
        } else {
            assert!(
                actual_output.contains(&expected_id_output),
                "actual output:   {}\nexpected output: {}\n",
                actual_output,
                expected_id_output
            );
        }
    }

    #[test]
    fn should_convert_from_event() {
        let _ = logging::init();
        let mut rng = crate::new_rng();

        // A valid `ApiVersion` SSE has no `id`, and should convert.
        let event = ServerSentEvent {
            id: None,
            data: SseData::random_api_version(&mut rng),
        };
        assert_event_converts(event);

        let id = rng.gen();
        let check = |data| {
            let id = Some(id);
            let event = ServerSentEvent { id, data };
            assert_event_converts(event);
        };

        // If an `ApiVersion` SSE has an `id`, the `id` should be ignored and the event should still
        // convert.
        check(SseData::random_api_version(&mut rng));

        // All other event variants should convert if provided with an `id`.
        check(SseData::random_block_added(&mut rng));
        check(SseData::random_deploy_processed(&mut rng));
        check(SseData::random_fault(&mut rng));
        check(SseData::random_finality_signature(&mut rng));
        check(SseData::random_step(&mut rng));
    }

    #[test]
    fn should_not_convert_from_event_with_no_id() {
        let _ = logging::init();
        let mut rng = crate::new_rng();

        let check = |data| {
            let id = None;
            let event = ServerSentEvent { id, data };
            let converted_event = WarpServerSentEvent::from(event);
            assert_eq!(converted_event.to_string(), "\n");
        };

        // All event variants other than `ApiVersion` should convert to a default warp event if not
        // provided with an `id`.  The `Display` impl for a default warp event is `\n`.
        check(SseData::random_block_added(&mut rng));
        check(SseData::random_deploy_processed(&mut rng));
        check(SseData::random_fault(&mut rng));
        check(SseData::random_finality_signature(&mut rng));
        check(SseData::random_step(&mut rng));
    }

    #[test]
    fn should_filter_events() {
        let _ = logging::init();
        let mut rng = crate::new_rng();

        let id = rng.gen();
        let should_filter_out = |data, filter| {
            let id = Some(id);
            let event = ServerSentEvent { id, data };
            assert!(filter_map_server_sent_event(event, filter).is_none());
        };

        let should_not_filter_out = |data, filter| {
            let id = Some(id);
            let event = ServerSentEvent { id, data };
            assert!(filter_map_server_sent_event(event, filter).is_some());
        };

        // `EventFilter::Main` should only filter out `FinalitySignature`s.
        let main_filter = EventFilter::Main;
        should_not_filter_out(SseData::random_api_version(&mut rng), main_filter);
        should_not_filter_out(SseData::random_block_added(&mut rng), main_filter);
        should_not_filter_out(SseData::random_deploy_processed(&mut rng), main_filter);
        should_not_filter_out(SseData::random_fault(&mut rng), main_filter);
        should_not_filter_out(SseData::random_step(&mut rng), main_filter);

        should_filter_out(SseData::random_finality_signature(&mut rng), main_filter);

        // `EventFilter::Signatures` should filter out everything except `ApiVersion`s and
        // `FinalitySignature`s.
        let sig_filter = EventFilter::Signatures;
        should_not_filter_out(SseData::random_api_version(&mut rng), sig_filter);
        should_not_filter_out(SseData::random_finality_signature(&mut rng), sig_filter);

        should_filter_out(SseData::random_block_added(&mut rng), sig_filter);
        should_filter_out(SseData::random_deploy_processed(&mut rng), sig_filter);
        should_filter_out(SseData::random_fault(&mut rng), sig_filter);
        should_filter_out(SseData::random_step(&mut rng), sig_filter);
    }
}
