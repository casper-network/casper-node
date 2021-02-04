use std::{convert::Infallible, time::Duration};

use futures::{
    future::{self, select},
    FutureExt,
};
use hyper::server::{conn::AddrIncoming, Builder};
use tokio::{
    select,
    sync::{mpsc, oneshot},
};
use tower::builder::ServiceBuilder;
use tracing::{info, trace};
use wheelbuf::WheelBuf;

use super::{
    sse_server::{self, BroadcastChannelMessage, ServerSentEvent, SSE_INITIAL_EVENT},
    Config, SseData,
};

/// Run the HTTP server.
///
/// `data_receiver` will provide the server with local events which should then be sent to all
/// subscribed clients.
pub(super) async fn run(
    config: Config,
    builder: Builder<AddrIncoming>,
    mut data_receiver: mpsc::UnboundedReceiver<SseData>,
) {
    // Event stream channels and filter.
    let (broadcaster, mut new_subscriber_info_receiver, sse_filter) =
        sse_server::create_channels_and_filter(config.broadcast_channel_size);

    let service = warp_json_rpc::service(sse_filter);

    // Start the server, passing a oneshot receiver to allow the server to be shut down gracefully.
    let make_svc =
        hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));

    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();

    let make_svc = ServiceBuilder::new()
        .rate_limit(config.qps_limit, Duration::from_secs(1))
        .service(make_svc);

    let server = builder.serve(make_svc);
    info!(address = %server.local_addr(), "started HTTP server");

    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });

    let server_joiner = tokio::spawn(server_with_shutdown);

    // Initialize the index and buffer for the SSEs.
    let mut event_index = 0_u32;
    let mut buffer = WheelBuf::new(vec![
        SSE_INITIAL_EVENT.clone();
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
                        let _ = subscriber.initial_events_sender.send(SSE_INITIAL_EVENT.clone());
                        // If the client supplied a "start_from" index, provide the buffered events.
                        // If they requested more than is buffered, just provide the whole buffer.
                        if let Some(start_index) = subscriber.start_from {
                            for event in buffer
                                .iter()
                                .skip_while(|event| event.id.unwrap() < start_index)
                            {
                                // As per sending `SSE_INITIAL_EVENT`, we don't care if this errors.
                                let _ = subscriber.initial_events_sender.send(event.clone());
                            }
                        }
                    }
                }

                maybe_data = data_receiver.recv() => {
                    match maybe_data {
                        Some(data) => {
                            // Buffer the data and broadcast it to subscribed clients.
                            trace!("Event stream server received {:?}", data);
                            let event = ServerSentEvent { id: Some(event_index), data };
                            buffer.push(event.clone());
                            let message = BroadcastChannelMessage::ServerSentEvent(event);
                            // This can validly fail if there are no connected clients, so don't log
                            // the error.
                            let _ = broadcaster.send(message);
                            event_index = event_index.wrapping_add(1);
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
    let _ = select(server_joiner, event_stream_fut.boxed()).await;

    // Kill the event-stream handlers, and shut down the server.
    let _ = broadcaster.send(BroadcastChannelMessage::Shutdown);
    let _ = shutdown_sender.send(());

    trace!("Event stream server stopped");
}
