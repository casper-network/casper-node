use futures::{future, Future, FutureExt};
use tokio::{
    select,
    sync::{broadcast, mpsc, oneshot},
    task,
};
use tracing::{debug, info, trace};
use wheelbuf::WheelBuf;

use casper_types::ProtocolVersion;

use super::{
    sse_server::{BroadcastChannelMessage, Id, NewSubscriberInfo, ServerSentEvent},
    Config, EventIndex, SseData,
};

/// Run the HTTP server.
///
/// * `server_with_shutdown` is the actual server as a future which can be gracefully shut down.
/// * `server_shutdown_sender` is the channel by which the server will be notified to shut down.
/// * `data_receiver` will provide the server with local events which should then be sent to all
///   subscribed clients.
/// * `broadcaster` is used by the server to send events to each subscribed client after receiving
///   them via the `data_receiver`.
/// * `new_subscriber_info_receiver` is used to notify the server of the details of a new client
///   having subscribed to the event stream.  It allows the server to populate that client's stream
///   with the requested number of historical events.
pub(super) async fn run(
    config: Config,
    api_version: ProtocolVersion,
    server_with_shutdown: impl Future<Output = ()> + Send + 'static,
    server_shutdown_sender: oneshot::Sender<()>,
    mut data_receiver: mpsc::UnboundedReceiver<(EventIndex, SseData)>,
    broadcaster: broadcast::Sender<BroadcastChannelMessage>,
    mut new_subscriber_info_receiver: mpsc::UnboundedReceiver<NewSubscriberInfo>,
) {
    let server_joiner = task::spawn(server_with_shutdown);

    // Initialize the index and buffer for the SSEs.
    let mut buffer = WheelBuf::new(vec![
        ServerSentEvent::initial_event(api_version);
        config.event_stream_buffer_length as usize
    ]);

    // Start handling received messages from the two channels; info on new client subscribers and
    // incoming events announced by node components.
    let event_stream_fut = async {
        loop {
            select! {
                maybe_new_subscriber = new_subscriber_info_receiver.recv() => {
                    if let Some(subscriber) = maybe_new_subscriber {
                        // First send the client the `ApiVersion` event.  We don't care if this
                        // errors - the client may have disconnected already.
                        let _ = subscriber
                            .initial_events_sender
                            .send(ServerSentEvent::initial_event(api_version));
                        // If the client supplied a "start_from" index, provide the buffered events.
                        // If they requested more than is buffered, just provide the whole buffer.
                        if let Some(start_index) = subscriber.start_from {
                            // If the buffer's first event ID is in the range [0, buffer size) or
                            // (Id::MAX - buffer size, Id::MAX], then the events in the buffer are
                            // considered to have their IDs wrapping round, or that was recently the
                            // case.  In this case, we add `buffer.capacity()` to `start_index` and
                            // the buffered events' IDs when considering which events to include in
                            // the requested initial events, effectively shifting all the IDs past
                            // the wrapping transition.
                            let buffer_size = buffer.capacity() as Id;
                            let in_wraparound_zone = buffer
                                .iter()
                                .next()
                                .map(|event| {
                                    let id = event.id.unwrap();
                                    id > Id::MAX - buffer_size || id < buffer_size
                                })
                                .unwrap_or_default();
                            for event in buffer.iter().skip_while(|event| {
                                if in_wraparound_zone {
                                    event.id.unwrap().wrapping_add(buffer_size)
                                        < start_index.wrapping_add(buffer_size)
                                } else {
                                    event.id.unwrap() < start_index
                                }
                            }) {
                                // As per sending `SSE_INITIAL_EVENT`, we don't care if this errors.
                                let _ = subscriber.initial_events_sender.send(event.clone());
                            }
                        }
                    }
                }

                maybe_data = data_receiver.recv() => {
                    match maybe_data {
                        Some((event_index, data)) => {
                            // Buffer the data and broadcast it to subscribed clients.
                            trace!("Event stream server received {:?}", data);
                            let event = ServerSentEvent { id: Some(event_index), data };
                            buffer.push(event.clone());
                            let message = BroadcastChannelMessage::ServerSentEvent(event);
                            // This can validly fail if there are no connected clients, so don't log
                            // the error.
                            let _ = broadcaster.send(message);
                        }
                        None => {
                            // The data sender has been dropped - exit the loop.
                            info!("shutting down HTTP server");
                            break;
                        }
                    }
                }
            }
        }
    };

    // Wait for the event stream future to exit, which will only happen if the last `data_sender`
    // paired with `data_receiver` is dropped.  `server_joiner` will never return here.
    let _ = future::select(server_joiner, event_stream_fut.boxed()).await;

    trace!("Right above shutdown");

    let shutdown_event = ServerSentEvent {
        id: None,
        data: SseData::Shutdown,
    };
    let shutdown_message = BroadcastChannelMessage::ServerSentEvent(shutdown_event);
    debug!("created the event");
    let _ = broadcaster.send(shutdown_message).expect("did not send?");
    // Kill the event-stream handlers, and shut down the server.
    let _ = broadcaster.send(BroadcastChannelMessage::Shutdown);
    let _ = server_shutdown_sender.send(());

    trace!("Event stream server stopped");
}
