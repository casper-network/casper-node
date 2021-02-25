//! Types and functions used by the http server to manage the event-stream.

use datasize::DataSize;
use futures::{Stream, StreamExt};
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::sync::{
    broadcast::{self, RecvError},
    mpsc,
};
use tracing::{error, info, trace};
use warp::{
    filters::BoxedFilter,
    sse::{self, ServerSentEvent as WarpServerSentEvent},
    Filter, Reply,
};

use casper_types::{ExecutionResult, PublicKey};

use crate::{
    components::consensus::EraId,
    types::{Block, BlockHash, DeployHash, FinalitySignature, TimeDiff, Timestamp},
};

/// The URL path.
pub const SSE_API_PATH: &str = "events";

/// The "id" field of the events sent on the event stream to clients.
type Id = u32;

/// The "data" field of the events sent on the event stream to clients.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub enum SseData {
    /// The version of this node's API server.  This event will always be the first sent to a new
    /// client, and will have no associated event ID provided.
    #[data_size(skip)]
    ApiVersion(Version),
    /// The given block has been added to the linear chain and stored locally.
    BlockAdded {
        block_hash: BlockHash,
        block: Box<Block>,
    },
    /// The given deploy has been executed, committed and forms part of the given block.
    DeployProcessed {
        deploy_hash: Box<DeployHash>,
        account: PublicKey,
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
    pub(super) fn initial_event(client_api_version: Version) -> Self {
        ServerSentEvent {
            id: None,
            data: SseData::ApiVersion(client_api_version),
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

/// Creates the message-passing channels required to run the event-stream server and the warp filter
/// for the event-stream server.
pub(super) fn create_channels_and_filter(
    broadcast_channel_size: usize,
) -> (
    broadcast::Sender<BroadcastChannelMessage>,
    mpsc::UnboundedReceiver<NewSubscriberInfo>,
    BoxedFilter<(impl Reply,)>,
) {
    // Create a channel to broadcast new events to all subscribed clients' streams.
    let (broadcaster, _) = broadcast::channel(broadcast_channel_size);
    let cloned_broadcaster = broadcaster.clone();

    // Create a channel for `NewSubscriberInfo`s to pass the information required to handle a new
    // client subscription.
    let (new_subscriber_info_sender, new_subscriber_info_receiver) = mpsc::unbounded_channel();

    let filter = warp::get()
        .and(warp::path(SSE_API_PATH))
        .and(warp::query().map(move |query: Query| {
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
            )))
        }))
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
fn stream_to_client(
    initial_events: mpsc::UnboundedReceiver<ServerSentEvent>,
    ongoing_events: broadcast::Receiver<BroadcastChannelMessage>,
) -> impl Stream<Item = Result<impl WarpServerSentEvent, RecvError>> + 'static {
    initial_events
        .map(|event| Ok(BroadcastChannelMessage::ServerSentEvent(event)))
        .chain(ongoing_events)
        .map(|result| {
            trace!(?result);
            match result {
                Ok(BroadcastChannelMessage::ServerSentEvent(event)) => {
                    match (event.id, &event.data) {
                        (None, &SseData::ApiVersion { .. }) => Ok(sse::json(event.data).boxed()),
                        (Some(id), &SseData::BlockAdded { .. })
                        | (Some(id), &SseData::DeployProcessed { .. })
                        | (Some(id), &SseData::FinalitySignature(_))
                        | (Some(id), &SseData::Fault { .. }) => {
                            Ok((sse::id(id), sse::json(event.data)).boxed())
                        }
                        _ => unreachable!("only ApiVersion may have no event ID"),
                    }
                }
                Ok(BroadcastChannelMessage::Shutdown) | Err(RecvError::Closed) => {
                    Err(RecvError::Closed)
                }
                Err(RecvError::Lagged(amount)) => {
                    info!(
                        "client lagged by {} events - dropping event stream connection to client",
                        amount
                    );
                    Err(RecvError::Lagged(amount))
                }
            }
        })
}
