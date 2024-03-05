//! Types and functions used by the http server to manage the event-stream.

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
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
    addr,
    filters::BoxedFilter,
    path,
    reject::Rejection,
    reply::Response,
    sse::{self, Event as WarpServerSentEvent},
    Filter, Reply,
};

use casper_types::{
    contract_messages::Messages,
    execution::{Effects, ExecutionResult},
    Block, BlockHash, EraId, FinalitySignature, InitiatorAddr, ProtocolVersion, PublicKey,
    TimeDiff, Timestamp, Transaction, TransactionHash,
};
#[cfg(test)]
use casper_types::{
    execution::ExecutionResultV2, testing::TestRng, Deploy, TestBlockBuilder, TransactionV1Builder,
};

/// The URL root path.
pub const SSE_API_PATH: &str = "events";
/// The URL query string field name.
pub const QUERY_FIELD: &str = "start_from";

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
        block: Box<Block>,
    },
    /// The given transaction has been newly-accepted by this node.
    TransactionAccepted {
        #[schemars(with = "Transaction", description = "a transaction")]
        transaction: Arc<Transaction>,
    },
    /// The given transaction has been executed, committed and forms part of the given block.
    TransactionProcessed {
        transaction_hash: Box<TransactionHash>,
        initiator_addr: Box<InitiatorAddr>,
        timestamp: Timestamp,
        ttl: TimeDiff,
        block_hash: Box<BlockHash>,
        //#[data_size(skip)]
        execution_result: Box<ExecutionResult>,
        messages: Messages,
    },
    /// The given transaction has expired.
    TransactionExpired { transaction_hash: TransactionHash },
    /// Generic representation of validator's fault in an era.
    Fault {
        era_id: EraId,
        public_key: Box<PublicKey>,
        timestamp: Timestamp,
    },
    /// New finality signature received.
    FinalitySignature(Box<FinalitySignature>),
    /// The execution effects produced by a `StepRequest`.
    Step {
        era_id: EraId,
        execution_effects: Effects,
    },
    /// The node is about to shut down.
    Shutdown,
}

#[cfg(test)]
impl SseData {
    /// Returns a random `SseData::BlockAdded`.
    pub(super) fn random_block_added(rng: &mut TestRng) -> Self {
        let block = TestBlockBuilder::new().build(rng);
        SseData::BlockAdded {
            block_hash: *block.hash(),
            block: Box::new(block.into()),
        }
    }

    /// Returns a random `SseData::TransactionAccepted`, along with the random `Transaction`.
    pub(super) fn random_transaction_accepted(rng: &mut TestRng) -> (Self, Transaction) {
        let txn = Transaction::random(rng);
        let event = SseData::TransactionAccepted {
            transaction: Arc::new(txn.clone()),
        };
        (event, txn)
    }

    /// Returns a random `SseData::TransactionProcessed`.
    pub(super) fn random_transaction_processed(rng: &mut TestRng) -> Self {
        let txn = Transaction::random(rng);
        let (timestamp, ttl) = match &txn {
            Transaction::Deploy(deploy) => (deploy.timestamp(), deploy.ttl()),
            Transaction::V1(txn) => (txn.timestamp(), txn.ttl()),
        };
        let message_count = rng.gen_range(0..6);
        let messages = std::iter::repeat_with(|| rng.gen())
            .take(message_count)
            .collect();

        SseData::TransactionProcessed {
            transaction_hash: Box::new(txn.hash()),
            initiator_addr: Box::new(txn.initiator_addr()),
            timestamp,
            ttl,
            block_hash: Box::new(BlockHash::random(rng)),
            execution_result: Box::new(ExecutionResult::from(ExecutionResultV2::random(rng))),
            messages,
        }
    }

    /// Returns a random `SseData::TransactionExpired`
    pub(super) fn random_transaction_expired(rng: &mut TestRng) -> Self {
        let timestamp = Timestamp::now() - TimeDiff::from_seconds(20);
        let ttl = TimeDiff::from_seconds(10);
        let txn = if rng.gen() {
            Transaction::from(Deploy::random_with_timestamp_and_ttl(rng, timestamp, ttl))
        } else {
            let txn = TransactionV1Builder::new_random(rng)
                .with_timestamp(timestamp)
                .with_ttl(ttl)
                .build()
                .unwrap();
            Transaction::from(txn)
        };

        SseData::TransactionExpired {
            transaction_hash: txn.hash(),
        }
    }

    /// Returns a random `SseData::Fault`.
    pub(super) fn random_fault(rng: &mut TestRng) -> Self {
        SseData::Fault {
            era_id: EraId::new(rng.gen()),
            public_key: Box::new(PublicKey::random(rng)),
            timestamp: Timestamp::random(rng),
        }
    }

    /// Returns a random `SseData::FinalitySignature`.
    pub(super) fn random_finality_signature(rng: &mut TestRng) -> Self {
        SseData::FinalitySignature(Box::new(FinalitySignature::random(rng)))
    }

    /// Returns a random `SseData::Step`.
    pub(super) fn random_step(rng: &mut TestRng) -> Self {
        let execution_effects = match ExecutionResultV2::random(rng) {
            ExecutionResultV2::Success { effects, .. }
            | ExecutionResultV2::Failure { effects, .. } => effects,
        };
        SseData::Step {
            era_id: EraId::new(rng.gen()),
            execution_effects,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct TransactionAccepted {
    pub(super) transaction_accepted: Arc<Transaction>,
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

/// Maps the `event` to a warp event, or `None` if it's a malformed event (ie.: `ApiVersion` event
/// with `id` set or event other than `ApiVersion` without `id`)
async fn map_server_sent_event(
    event: &ServerSentEvent,
) -> Option<Result<WarpServerSentEvent, RecvError>> {
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
        | &SseData::TransactionProcessed { .. }
        | &SseData::TransactionExpired { .. }
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

        SseData::TransactionAccepted { transaction } => Some(Ok(WarpServerSentEvent::default()
            .json_data(&TransactionAccepted {
                transaction_accepted: Arc::clone(transaction),
            })
            .unwrap_or_else(|error| {
                warn!(%error, "failed to jsonify sse event");
                WarpServerSentEvent::default()
            })
            .id(event.id.unwrap().to_string()))),
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
        "invalid path: expected '/{root}'\n",
        root = SSE_API_PATH,
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

        let serve = move |query: HashMap<String, String>,
                          maybe_remote_address: Option<SocketAddr>| {
            let remote_address = match maybe_remote_address {
                Some(address) => address.to_string(),
                None => "unknown".to_string(),
            };

            // If we already have the maximum number of subscribers, reject this new one.
            if cloned_broadcaster.receiver_count() >= max_concurrent_subscribers as usize {
                info!(
                    %remote_address,
                    %max_concurrent_subscribers,
                    "event stream server has max subscribers: rejecting new one"
                );
                return create_503();
            }

            let start_from = match parse_query(query) {
                Ok(maybe_id) => maybe_id,
                Err(error_response) => return error_response,
            };

            // Create a channel for the client's handler to receive the stream of initial events.
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

            // Create a channel for the client's handler to receive the stream of ongoing events.
            let ongoing_events_receiver = cloned_broadcaster.subscribe();

            sse::reply(sse::keep_alive().stream(stream_to_client(
                initial_events_receiver,
                ongoing_events_receiver,
                remote_address,
            )))
            .into_response()
        };

        let sse_filter = warp::get()
            .and(path(SSE_API_PATH))
            .and(path::end())
            .and(warp::query())
            .and(addr::remote())
            .map(serve)
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
    remote_address: String,
) -> impl Stream<Item = Result<WarpServerSentEvent, RecvError>> + 'static {
    // Keep a record of the IDs of the events delivered via the `initial_events` receiver.
    let initial_stream_ids = Arc::new(RwLock::new(HashSet::new()));
    let cloned_initial_ids = Arc::clone(&initial_stream_ids);

    // Map the events arriving after the initial stream to the correct error type, filtering out any
    // that have already been sent in the initial stream.
    let ongoing_stream = BroadcastStream::new(ongoing_events)
        .filter_map(move |result| {
            let cloned_initial_ids = Arc::clone(&cloned_initial_ids);
            let remote_address = remote_address.clone();
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
                    Err(BroadcastStreamRecvError::Lagged(lagged_count)) => {
                        info!(
                            %remote_address,
                            %lagged_count,
                            "client lagged: dropping event stream connection to client",
                        );
                        Some(Err(RecvError::Lagged(lagged_count)))
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
                Ok(event) => map_server_sent_event(&event).await,
                Err(error) => Some(Err(error)),
            }
        })
}

#[cfg(test)]
mod tests {
    use std::iter;

    use casper_types::testing::TestRng;

    use super::*;
    use crate::logging;

    /// This test checks that events from the initial stream which are duplicated in the
    /// ongoing stream are filtered out.
    #[tokio::test]
    async fn should_filter_duplicate_events() {
        // Returns `count` SSE events. The events will have sequential IDs starting from `start_id`.
        fn make_events(rng: &mut TestRng, start_id: Id, count: usize) -> Vec<ServerSentEvent> {
            (start_id..(start_id + count as u32))
                .map(|id| ServerSentEvent {
                    id: Some(id),
                    data: SseData::random_finality_signature(rng),
                })
                .collect()
        }

        // Returns `NUM_ONGOING_EVENTS` SSE events containing duplicates taken from the end of the
        // initial stream.  Allows for the full initial stream to be duplicated except for
        // its first event (the `ApiVersion` one) which has no ID.
        fn make_ongoing_events(
            rng: &mut TestRng,
            duplicate_count: usize,
            initial_events: &[ServerSentEvent],
        ) -> Vec<ServerSentEvent> {
            assert!(duplicate_count < initial_events.len());
            let initial_skip_count = initial_events.len() - duplicate_count;
            let unique_start_id = initial_events.len() as Id - 1;
            let unique_count = NUM_ONGOING_EVENTS - duplicate_count;
            initial_events
                .iter()
                .skip(initial_skip_count)
                .cloned()
                .chain(make_events(rng, unique_start_id, unique_count))
                .collect()
        }

        // The number of events in the initial stream, excluding the very first `ApiVersion` one.
        const NUM_INITIAL_EVENTS: usize = 10;
        // The number of events in the ongoing stream, including any duplicated from the initial
        // stream.
        const NUM_ONGOING_EVENTS: usize = 20;

        let _ = logging::init();
        let mut rng = crate::new_rng();

        let initial_events: Vec<ServerSentEvent> =
            iter::once(ServerSentEvent::initial_event(ProtocolVersion::V1_0_0))
                .chain(make_events(&mut rng, 0, NUM_INITIAL_EVENTS))
                .collect();

        // Run three cases; where only a single event is duplicated, where five are duplicated, and
        // where the whole initial stream (except the `ApiVersion`) is duplicated.
        for duplicate_count in &[1, 5, NUM_INITIAL_EVENTS] {
            // Create the events with the requisite duplicates at the start of the collection.
            let ongoing_events = make_ongoing_events(&mut rng, *duplicate_count, &initial_events);

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
                "127.0.0.1:3456".to_string(),
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
                let expected_data_string = serde_json::to_string(&deduplicated_event.data).unwrap();

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
}
