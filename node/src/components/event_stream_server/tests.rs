use std::{
    cell::RefCell,
    error::Error,
    fs, io, iter, str,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use futures::{join, StreamExt};
use http::StatusCode;
use pretty_assertions::assert_eq;
use rand::Rng;
use schemars::schema_for;
use tempfile::TempDir;
use tokio::{
    sync::{Barrier, Notify},
    task::JoinHandle,
    time,
};
use tracing::debug;

use super::*;
use crate::{logging, testing::TestRng};
use sse_server::{
    EventFilter, Id, QUERY_FIELD, SSE_API_MAIN_PATH, SSE_API_ROOT_PATH, SSE_API_SIGNATURES_PATH,
};

/// The total number of random events each `EventStreamServer` will emit by default, excluding the
/// initial `ApiVersion` event.
const EVENT_COUNT: u32 = 100;
/// The event stream buffer length, set in the server's config.  Set to half of the total event
/// count to allow for the buffer purging events in the test.
const BUFFER_LENGTH: u32 = EVENT_COUNT / 2;
/// The maximum amount of time to wait for a test server to complete.  If this time is exceeded, the
/// test has probably hung, and should be deemed to have failed.
const MAX_TEST_TIME: Duration = Duration::from_secs(1);
/// The duration of the sleep called between each event being sent by the server.
const DELAY_BETWEEN_EVENTS: Duration = Duration::from_millis(1);

thread_local!(
    /// A per-test identifier for the clients.
    static CLIENT_ID: RefCell<usize> = RefCell::new(0)
);

/// A helper to allow the synchronization of a single client joining the SSE server.
///
/// It provides the primitives to allow the client to connect to the server just before a specific
/// event is emitted by the server.
#[derive(Clone)]
struct ClientSyncBehavior {
    /// The event ID before which the server should wait at the barrier for the client to join.
    join_before_event: Id,
    /// The barrier to sync the client joining the server.
    barrier: Arc<Barrier>,
}

impl ClientSyncBehavior {
    fn new(join_before_event: Id) -> (Self, Arc<Barrier>) {
        let barrier = Arc::new(Barrier::new(2));
        let behavior = ClientSyncBehavior {
            join_before_event,
            barrier: Arc::clone(&barrier),
        };
        (behavior, barrier)
    }
}

/// A helper to allow the synchronization of all clients joining a single SSE server.
#[derive(Clone)]
struct ServerSyncBehavior {
    /// Whether the server should have a delay between sending events, to allow a client to keep up
    /// and not be disconnected for lagging.
    has_delay_between_events: bool,
    clients: Vec<ClientSyncBehavior>,
}

impl ServerSyncBehavior {
    /// Returns a new `ServerSyncBehavior` with a 1 millisecond delay per event, and no clients.
    fn new() -> Self {
        ServerSyncBehavior {
            has_delay_between_events: true,
            clients: Vec::new(),
        }
    }

    /// Removes the delay between events.
    fn with_no_delay_between_events(mut self) -> Self {
        self.has_delay_between_events = false;
        self
    }

    /// Adds a client sync behavior, specified for the client to connect to the server just before
    /// `id` is emitted.
    fn add_client_sync_before_event(&mut self, id: Id) -> Arc<Barrier> {
        let (client_behavior, barrier) = ClientSyncBehavior::new(id);
        self.clients.push(client_behavior);
        barrier
    }

    /// Waits for all clients which specified they wanted to join just before the given event ID.
    async fn wait_for_clients(&self, id: Id) {
        for client_behavior in &self.clients {
            if client_behavior.join_before_event == id {
                debug!("server waiting before event {}", id);
                client_behavior.barrier.wait().await;
                debug!("server waiting for client to connect before event {}", id);
                client_behavior.barrier.wait().await;
                debug!("server finished waiting before event {}", id);
            }
        }
    }

    /// Sleeps if `self` was set to enable delays between events.
    async fn sleep_if_required(&self) {
        if self.has_delay_between_events {
            time::sleep(DELAY_BETWEEN_EVENTS).await;
        }
    }
}

/// A helper to allow the server to be kept alive until a specific call to stop it.
#[derive(Clone)]
struct ServerStopper {
    should_stop: Arc<AtomicBool>,
    notifier: Arc<Notify>,
}

impl ServerStopper {
    fn new() -> Self {
        ServerStopper {
            should_stop: Arc::new(AtomicBool::new(false)),
            notifier: Arc::new(Notify::new()),
        }
    }

    /// Returns whether the server should stop now or not.
    fn should_stop(&self) -> bool {
        self.should_stop.load(Ordering::SeqCst)
    }

    /// Waits until the server should stop.
    async fn wait(&self) {
        while !self.should_stop() {
            self.notifier.notified().await;
        }
    }

    /// Tells the server to stop.
    fn stop(&self) {
        self.should_stop.store(true, Ordering::SeqCst);
        self.notifier.notify_one();
    }
}

impl Drop for ServerStopper {
    fn drop(&mut self) {
        self.stop();
    }
}

struct TestFixture {
    storage_dir: TempDir,
    protocol_version: ProtocolVersion,
    events: Vec<SseData>,
    first_event_id: Id,
    server_join_handle: Option<JoinHandle<()>>,
    server_stopper: ServerStopper,
}

impl TestFixture {
    /// Constructs a new `TestFixture` including `EVENT_COUNT` random events ready to be served.
    fn new(rng: &mut TestRng) -> Self {
        let _ = logging::init();
        let storage_dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(&storage_dir).unwrap();
        let protocol_version = ProtocolVersion::from_parts(1, 2, 3);

        let events = (0..EVENT_COUNT)
            .map(|_| match rng.gen_range(0..8) {
                0 => SseData::random_block_added(rng),
                1 => SseData::random_deploy_processed(rng),
                2 => SseData::random_fault(rng),
                3 => SseData::random_step(rng),
                4..=7 => SseData::random_finality_signature(rng),
                _ => unreachable!(),
            })
            .collect();

        TestFixture {
            storage_dir,
            protocol_version,
            events,
            first_event_id: 0,
            server_join_handle: None,
            server_stopper: ServerStopper::new(),
        }
    }

    /// Creates a new `EventStreamServer` and runs it in a tokio task, returning the actual address
    /// the server is listening on.
    ///
    /// Only one server can be run at a time; this panics if there is already a server task running.
    ///
    /// The server emits a clone of each of the random events held by the `TestFixture`, in the
    /// order in which they're held in the `TestFixture`.
    ///
    /// The server runs until `TestFixture::stop_server()` is called, or the `TestFixture` is
    /// dropped.
    async fn run_server(&mut self, sync_behavior: ServerSyncBehavior) -> SocketAddr {
        if self.server_join_handle.is_some() {
            panic!("one `TestFixture` can only run one server at a time");
        }
        self.server_stopper = ServerStopper::new();

        // Set the server to use a channel buffer of half the total events it will emit.
        let config = Config {
            event_stream_buffer_length: BUFFER_LENGTH,
            ..Default::default()
        };
        let mut server = EventStreamServer::new(
            config,
            self.storage_dir.path().to_path_buf(),
            self.protocol_version,
        )
        .unwrap();

        self.first_event_id = server.event_indexer.current_index();

        let first_event_id = server.event_indexer.current_index();
        let server_address = server.listening_address;
        let events = self.events.clone();
        let server_stopper = self.server_stopper.clone();

        let join_handle = tokio::spawn(async move {
            for (id, event) in events.into_iter().enumerate() {
                if server_stopper.should_stop() {
                    debug!("stopping server early");
                    return;
                }
                sync_behavior
                    .wait_for_clients((id as Id).wrapping_add(first_event_id))
                    .await;
                let _ = server.broadcast(event);
                sync_behavior.sleep_if_required().await;
            }

            // Keep the server running until told to stop.  Clients connecting from now will only
            // receive keepalives.
            debug!("server finished sending all events");
            server_stopper.wait().await;
            debug!("server stopped");
        });

        self.server_join_handle = Some(join_handle);

        server_address
    }

    /// Stops the currently-running server, if any, panicking if unable to stop the server within
    /// `MAX_TEST_TIME`.
    ///
    /// Must be called and awaited before starting a new server with this particular `TestFixture`.
    ///
    /// Should be called in every test where a server has been started, since this will ensure
    /// failed tests won't hang indefinitely.
    async fn stop_server(&mut self) {
        let join_handle = match self.server_join_handle.take() {
            Some(join_handle) => join_handle,
            None => return,
        };
        self.server_stopper.stop();
        time::timeout(MAX_TEST_TIME, join_handle)
            .await
            .expect("stopping server timed out (test hung)")
            .expect("server task should not error");
    }

    /// Returns all the events which would have been received by a client via the `/events/main` URL
    /// or the `/events/sigs` URL depending on `filter`, where the client connected just before
    /// `from` was emitted from the server.  This includes the initial `ApiVersion` event.
    fn filtered_events(&self, filter: EventFilter, from: Id) -> Vec<ReceivedEvent> {
        // Convert the IDs to `u128`s to cater for wrapping and add `Id::MAX + 1` to `from` if the
        // buffer wrapped and `from` represents an event from after the wrap.
        let threshold = Id::MAX - EVENT_COUNT;
        let from = if self.first_event_id >= threshold && from < threshold {
            from as u128 + Id::MAX as u128 + 1
        } else {
            from as u128
        };

        let id_filter = |id: u128, event: &SseData| -> Option<ReceivedEvent> {
            if id < from {
                return None;
            }
            Some(ReceivedEvent {
                id: Some(id as Id),
                data: serde_json::to_string(event).unwrap(),
            })
        };

        let api_version_event = ReceivedEvent {
            id: None,
            data: serde_json::to_string(&SseData::ApiVersion(self.protocol_version)).unwrap(),
        };

        iter::once(api_version_event)
            .chain(self.events.iter().enumerate().filter_map(|(id, event)| {
                let id = id as u128 + self.first_event_id as u128;
                match event {
                    SseData::BlockAdded { .. }
                    | SseData::DeployProcessed { .. }
                    | SseData::Fault { .. }
                    | SseData::Step { .. } => match filter {
                        EventFilter::Main => id_filter(id, event),
                        EventFilter::Signatures => None,
                    },
                    SseData::FinalitySignature(_) => match filter {
                        EventFilter::Main => None,
                        EventFilter::Signatures => id_filter(id, event),
                    },
                    SseData::ApiVersion(_) => {
                        panic!("should not hold ApiVersion event in test fixture")
                    }
                }
            }))
            .collect()
    }

    /// Returns all the events which would have been received by a client connected from server
    /// startup via the `/events/main` URL or the `/events/sigs` URL depending on `filter`,
    /// including the initial `ApiVersion` event.
    fn all_filtered_events(&self, filter: EventFilter) -> Vec<ReceivedEvent> {
        self.filtered_events(filter, self.first_event_id)
    }
}

/// Returns the URL for a client to use to connect to the server at the given address.
///
/// Uses `events/main` or `events/sigs` depending upon the value of `filter`, and appends a
/// `?start_from=X` query string if `maybe_start_from` is `Some`.
fn url(server_address: SocketAddr, filter: EventFilter, maybe_start_from: Option<Id>) -> String {
    format!(
        "http://{}/{}/{}{}",
        server_address,
        SSE_API_ROOT_PATH,
        match filter {
            EventFilter::Main => SSE_API_MAIN_PATH,
            EventFilter::Signatures => SSE_API_SIGNATURES_PATH,
        },
        match maybe_start_from {
            Some(start_from) => format!("?{}={}", QUERY_FIELD, start_from),
            None => String::new(),
        }
    )
}

/// The representation of an SSE event as received by a subscribed client.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ReceivedEvent {
    id: Option<Id>,
    data: String,
}

/// Runs a client, consuming all SSE events until the server starts emitting keepalive chars `:`.
///
/// The client waits at the barrier before connecting to the server, and then again immediately
/// after connecting to ensure the server doesn't start sending events before the client is
/// connected.
async fn subscribe(url: &str, barrier: Arc<Barrier>) -> Result<Vec<ReceivedEvent>, reqwest::Error> {
    let client_id = CLIENT_ID.with(|client_id| {
        let mut id = client_id.borrow_mut();
        let copied_id = *id;
        *id = copied_id + 1;
        format!("client {}", copied_id)
    });

    debug!("{} waiting before connecting via {}", client_id, url);
    barrier.wait().await;
    let response = reqwest::get(url).await?;
    debug!("{} waiting after connecting", client_id);
    barrier.wait().await;
    debug!("{} finished waiting", client_id);

    // The stream from the server is not always chunked into events, so gather the stream into a
    // single `String` until we receive a keepalive.
    let mut response_text = String::new();
    let mut stream = response.bytes_stream();
    while let Some(item) = stream.next().await {
        // If the server crashes or returns an error in the stream, it is caught here as `item` will
        // be an `Err`.
        let bytes = item?;
        let chunk = str::from_utf8(bytes.as_ref()).unwrap();
        response_text.push_str(chunk);
        if response_text.lines().rev().any(|line| line == ":") {
            debug!("{} received keepalive: exiting", client_id);
            break;
        }
    }

    // Iterate the lines of the response body.  Each line should be one of
    //   * an SSE event: line starts with "data:" and the remainder of the line is a JSON object
    //   * an SSE event ID: line starts with "id:" and the remainder is a decimal encoded `u32`
    //   * empty
    //   * a keepalive: line contains exactly ":"
    //
    // The expected order is:
    //   * data:<JSON-encoded ApiVersion> (note, no ID line follows this first event)
    // then the following three repeated for as many events as are applicable to that stream:
    //   * data:<JSON-encoded event>
    //   * id:<integer>
    //   * empty line
    // then finally, repeated keepalive lines until the server is shut down.
    let mut received_events = Vec::new();
    let mut line_itr = response_text.lines();
    while let Some(data_line) = line_itr.next() {
        let data = match data_line.strip_prefix("data:") {
            Some(data_str) => data_str.to_string(),
            None => {
                if data_line.trim().is_empty() || data_line.trim() == ":" {
                    continue;
                } else {
                    panic!(
                        "{}: data line should start with 'data:'\n{}",
                        client_id, data_line
                    )
                }
            }
        };

        let id_line = match line_itr.next() {
            Some(line) => line,
            None => break,
        };

        let id = match id_line.strip_prefix("id:") {
            Some(id_str) => Some(id_str.parse().unwrap_or_else(|_| {
                panic!("{}: failed to get ID line from:\n{}", client_id, id_line)
            })),
            None => {
                if id_line.trim().is_empty() && received_events.is_empty() {
                    None
                } else if id_line.trim() == ":" {
                    continue;
                } else {
                    panic!(
                        "{}: every event must have an ID except the first one",
                        client_id
                    );
                }
            }
        };

        received_events.push(ReceivedEvent { id, data });
    }

    Ok(received_events)
}

/// Client setup:
///   * `<IP:port>/events/main` or `<IP:port>/events/sigs` depending on `filter`
///   * no `?start_from=` query
///   * connected before first event
///
/// Expected to receive all main or signature events depending on `filter`.
async fn should_serve_events_with_no_query(filter: EventFilter) {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    let mut sync_behavior = ServerSyncBehavior::new();
    let barrier = sync_behavior.add_client_sync_before_event(0);
    let server_address = fixture.run_server(sync_behavior).await;

    let url = url(server_address, filter, None);
    let received_events = subscribe(&url, barrier).await.unwrap();
    fixture.stop_server().await;

    assert_eq!(received_events, fixture.all_filtered_events(filter));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_serve_main_events_with_no_query() {
    should_serve_events_with_no_query(EventFilter::Main).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_serve_signature_events_with_no_query() {
    should_serve_events_with_no_query(EventFilter::Signatures).await;
}

/// Client setup:
///   * `<IP:port>/events/main?start_from=25` or `<IP:port>/events/sigs?start_from=25` depending on
///     `filter`
///   * connected just before event ID 50
///
/// Expected to receive main or signature events (depending on `filter`) from ID 25 onwards, as
/// events 25 to 49 should still be in the server buffer.
async fn should_serve_events_with_query(filter: EventFilter) {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    let connect_at_event_id = BUFFER_LENGTH;
    let start_from_event_id = BUFFER_LENGTH / 2;

    let mut sync_behavior = ServerSyncBehavior::new();
    let barrier = sync_behavior.add_client_sync_before_event(connect_at_event_id);
    let server_address = fixture.run_server(sync_behavior).await;

    let url = url(server_address, filter, Some(start_from_event_id));
    let received_events = subscribe(&url, barrier).await.unwrap();
    fixture.stop_server().await;

    let expected_events = fixture.filtered_events(filter, start_from_event_id);
    assert_eq!(received_events, expected_events);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_serve_main_events_with_query() {
    should_serve_events_with_query(EventFilter::Main).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_serve_signature_events_with_query() {
    should_serve_events_with_query(EventFilter::Signatures).await;
}

/// Client setup:
///   * `<IP:port>/events/main?start_from=0` or `<IP:port>/events/sigs?start_from=0` depending on
///     `filter`
///   * connected just before event ID 75
///
/// Expected to receive main or signature events (depending on `filter`) from ID 25 onwards, as
/// events 0 to 24 should have been purged from the server buffer.
async fn should_serve_remaining_events_with_query(filter: EventFilter) {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    let connect_at_event_id = BUFFER_LENGTH * 3 / 2;
    let start_from_event_id = 0;

    let mut sync_behavior = ServerSyncBehavior::new();
    let barrier = sync_behavior.add_client_sync_before_event(connect_at_event_id);
    let server_address = fixture.run_server(sync_behavior).await;

    let url = url(server_address, filter, Some(start_from_event_id));
    let received_events = subscribe(&url, barrier).await.unwrap();
    fixture.stop_server().await;

    let expected_first_event = connect_at_event_id - BUFFER_LENGTH;
    let expected_events = fixture.filtered_events(filter, expected_first_event);
    assert_eq!(received_events, expected_events);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_serve_remaining_main_events_with_query() {
    should_serve_remaining_events_with_query(EventFilter::Main).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_serve_remaining_signature_events_with_query() {
    should_serve_remaining_events_with_query(EventFilter::Signatures).await;
}

/// Client setup:
///   * `<IP:port>/events/main?start_from=25` or `<IP:port>/events/sigs?start_from=25` depending on
///     `filter`
///   * connected before first event
///
/// Expected to receive all main or signature events (depending on `filter`), as event 25 hasn't
/// been added to the server buffer yet.
async fn should_serve_events_with_query_for_future_event(filter: EventFilter) {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    let mut sync_behavior = ServerSyncBehavior::new();
    let barrier = sync_behavior.add_client_sync_before_event(0);
    let server_address = fixture.run_server(sync_behavior).await;

    let url = url(server_address, filter, Some(25));
    let received_events = subscribe(&url, barrier).await.unwrap();
    fixture.stop_server().await;

    assert_eq!(received_events, fixture.all_filtered_events(filter));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_serve_main_events_with_query_for_future_event() {
    should_serve_events_with_query_for_future_event(EventFilter::Main).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_serve_signature_events_with_query_for_future_event() {
    should_serve_events_with_query_for_future_event(EventFilter::Signatures).await;
}

/// Checks that when a server is shut down (e.g. for a node upgrade), connected clients don't have
/// an error while handling the HTTP response.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_exit_should_gracefully_shut_down_stream() {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    // Start the server, waiting for two clients to connect.
    let mut sync_behavior = ServerSyncBehavior::new();
    let barrier1 = sync_behavior.add_client_sync_before_event(0);
    let barrier2 = sync_behavior.add_client_sync_before_event(0);
    let server_address = fixture.run_server(sync_behavior).await;

    let url1 = url(server_address, EventFilter::Main, None);
    let url2 = url(server_address, EventFilter::Signatures, None);

    // Run the two clients, and stop the server after a short delay.
    let (received_events1, received_events2, _) = join!(
        subscribe(&url1, barrier1),
        subscribe(&url2, barrier2),
        async {
            time::sleep(DELAY_BETWEEN_EVENTS * EVENT_COUNT / 2).await;
            fixture.stop_server().await
        }
    );

    // Ensure both clients' streams terminated without error.
    let received_events1 = received_events1.unwrap();
    let received_events2 = received_events2.unwrap();

    // Ensure both clients received some events...
    assert!(!received_events1.is_empty());
    assert!(!received_events2.is_empty());

    // ...but not the full set they would have if the server hadn't stopped early.
    assert!(received_events1.len() < fixture.all_filtered_events(EventFilter::Main).len());
    assert!(received_events2.len() < fixture.all_filtered_events(EventFilter::Signatures).len());
}

/// Checks that clients which don't consume the events in a timely manner are forcibly disconnected
/// by the server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lagging_clients_should_be_disconnected() {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    // Start the server, waiting for two clients to connect.  Set the server to have no delay
    // between sending events, meaning the clients will lag.
    let mut sync_behavior = ServerSyncBehavior::new().with_no_delay_between_events();
    let barrier1 = sync_behavior.add_client_sync_before_event(0);
    let barrier2 = sync_behavior.add_client_sync_before_event(0);
    let server_address = fixture.run_server(sync_behavior).await;

    let url1 = url(server_address, EventFilter::Main, None);
    let url2 = url(server_address, EventFilter::Signatures, None);

    // Run the two clients, then stop the server.
    let (result1, result2) = join!(subscribe(&url1, barrier1), subscribe(&url2, barrier2),);
    fixture.stop_server().await;

    // Ensure both clients' streams terminated with an `UnexpectedEof` error.
    let check_error = |result: Result<_, reqwest::Error>| {
        let kind = result
            .unwrap_err()
            .source()
            .expect("reqwest::Error should have source")
            .downcast_ref::<hyper::Error>()
            .expect("reqwest::Error's source should be a hyper::Error")
            .source()
            .expect("hyper::Error should have source")
            .downcast_ref::<io::Error>()
            .expect("hyper::Error's source should be a std::io::Error")
            .kind();
        assert!(matches!(kind, io::ErrorKind::UnexpectedEof));
    };
    check_error(result1);
    check_error(result2);
}

/// Checks that clients using the correct <IP:Port> but wrong path get a helpful error response.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_handle_bad_url_path() {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    let server_address = fixture.run_server(ServerSyncBehavior::new()).await;

    #[rustfmt::skip]
    let urls = [
        format!("http://{}", server_address),
        format!("http://{}?{}=0", server_address, QUERY_FIELD),
        format!("http://{}/bad", server_address),
        format!("http://{}/bad?{}=0", server_address, QUERY_FIELD),
        format!("http://{}/{}", server_address, SSE_API_ROOT_PATH),
        format!("http://{}/{}?{}=0", server_address, QUERY_FIELD, SSE_API_ROOT_PATH),
        format!("http://{}/{}/bad", server_address, SSE_API_ROOT_PATH),
        format!("http://{}/{}/bad?{}=0", server_address, QUERY_FIELD, SSE_API_ROOT_PATH),
        format!("http://{}/{}/{}bad", server_address, SSE_API_ROOT_PATH, SSE_API_MAIN_PATH),
        format!("http://{}/{}/{}bad?{}=0", server_address, QUERY_FIELD, SSE_API_ROOT_PATH, SSE_API_MAIN_PATH),
        format!("http://{}/{}/{}bad", server_address, SSE_API_ROOT_PATH, SSE_API_SIGNATURES_PATH),
        format!("http://{}/{}/{}bad?{}=0", server_address, QUERY_FIELD, SSE_API_ROOT_PATH, SSE_API_SIGNATURES_PATH),
        format!("http://{}/{}/{}/bad", server_address, SSE_API_ROOT_PATH, SSE_API_MAIN_PATH),
        format!("http://{}/{}/{}/bad?{}=0", server_address, QUERY_FIELD, SSE_API_ROOT_PATH, SSE_API_MAIN_PATH),
        format!("http://{}/{}/{}/bad", server_address, SSE_API_ROOT_PATH, SSE_API_SIGNATURES_PATH),
        format!("http://{}/{}/{}/bad?{}=0", server_address, QUERY_FIELD, SSE_API_ROOT_PATH, SSE_API_SIGNATURES_PATH),
    ];

    let expected_body = format!(
        "invalid path: expected '{0}/{1}' or '{0}/{2}'",
        SSE_API_ROOT_PATH, SSE_API_MAIN_PATH, SSE_API_SIGNATURES_PATH
    );
    for url in &urls {
        let response = reqwest::get(url).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "URL: {}", url);
        assert_eq!(
            response.text().await.unwrap().trim(),
            &expected_body,
            "URL: {}",
            url
        );
    }

    fixture.stop_server().await;
}

/// Checks that clients using the correct <IP:Port/path> but wrong query get a helpful error
/// response.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_handle_bad_url_query() {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    let server_address = fixture.run_server(ServerSyncBehavior::new()).await;

    let main_url = format!(
        "http://{}/{}/{}",
        server_address, SSE_API_ROOT_PATH, SSE_API_MAIN_PATH
    );
    let sigs_url = format!(
        "http://{}/{}/{}",
        server_address, SSE_API_ROOT_PATH, SSE_API_SIGNATURES_PATH
    );
    let urls = [
        format!("{}?not-a-kv-pair", main_url),
        format!("{}?not-a-kv-pair", sigs_url),
        format!("{}?start_fro=0", main_url),
        format!("{}?start_fro=0", sigs_url),
        format!("{}?{}=not-integer", main_url, QUERY_FIELD),
        format!("{}?{}=not-integer", sigs_url, QUERY_FIELD),
        format!("{}?{}='0'", main_url, QUERY_FIELD),
        format!("{}?{}='0'", sigs_url, QUERY_FIELD),
        format!("{}?{}=0&extra=1", main_url, QUERY_FIELD),
        format!("{}?{}=0&extra=1", sigs_url, QUERY_FIELD),
    ];

    let expected_body = format!(
        "invalid query: expected single field '{}=<EVENT ID>'",
        QUERY_FIELD
    );
    for url in &urls {
        let response = reqwest::get(url).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "URL: {}",
            url
        );
        assert_eq!(
            response.text().await.unwrap().trim(),
            &expected_body,
            "URL: {}",
            url
        );
    }

    fixture.stop_server().await;
}

/// Check that a server which restarts continues from the previous numbering of event IDs.
async fn should_persist_event_ids(filter: EventFilter) {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    {
        // Run the first server to emit the 100 events.
        let mut sync_behavior = ServerSyncBehavior::new();
        let barrier = sync_behavior.add_client_sync_before_event(0);
        let server_address = fixture.run_server(sync_behavior).await;

        // Consume these and stop the server.
        let url = url(server_address, filter, None);
        let _ = subscribe(&url, barrier).await.unwrap();
        fixture.stop_server().await;
    }

    {
        // Start a new server with a client barrier set for just before event ID 100.
        let mut sync_behavior = ServerSyncBehavior::new();
        let barrier = sync_behavior.add_client_sync_before_event(EVENT_COUNT);
        let server_address = fixture.run_server(sync_behavior).await;

        // Check the test fixture has set the server's first event ID to 100.
        assert_eq!(fixture.first_event_id, EVENT_COUNT);

        // Consume the events and assert their IDs are all >= 100.
        let url = url(server_address, filter, None);
        let received_events = subscribe(&url, barrier).await.unwrap();
        fixture.stop_server().await;

        assert_eq!(received_events, fixture.all_filtered_events(filter));
        assert!(received_events
            .iter()
            .skip(1)
            .all(|event| event.id.unwrap() >= EVENT_COUNT));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_persist_main_event_ids() {
    should_persist_event_ids(EventFilter::Main).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_persist_signature_event_ids() {
    should_persist_event_ids(EventFilter::Signatures).await;
}

/// Check that a server handles wrapping round past the maximum value for event IDs.
async fn should_handle_wrapping_past_max_event_id(filter: EventFilter) {
    let mut rng = crate::new_rng();
    let mut fixture = TestFixture::new(&mut rng);

    // Set up an `EventIndexer` cache file as if the server previously stopped at an event with ID
    // just less than the maximum.
    let start_index = Id::MAX - (BUFFER_LENGTH / 2);
    fs::write(
        fixture.storage_dir.path().join("sse_index"),
        start_index.to_le_bytes(),
    )
    .unwrap();

    // Set up a client which will connect at the start of the stream, and another two for once the
    // IDs have wrapped past the maximum value.
    let mut sync_behavior = ServerSyncBehavior::new();
    let barrier1 = sync_behavior.add_client_sync_before_event(start_index);
    let barrier2 = sync_behavior.add_client_sync_before_event(BUFFER_LENGTH / 2);
    let barrier3 = sync_behavior.add_client_sync_before_event(BUFFER_LENGTH / 2);
    let server_address = fixture.run_server(sync_behavior).await;
    assert_eq!(fixture.first_event_id, start_index);

    // The first client doesn't need a query string, but the second will request to start from an ID
    // from before they wrapped past the maximum value, and the third from event 0.
    let url1 = url(server_address, filter, None);
    let url2 = url(server_address, filter, Some(start_index + 1));
    let url3 = url(server_address, filter, Some(0));
    let (received_events1, received_events2, received_events3) = join!(
        subscribe(&url1, barrier1),
        subscribe(&url2, barrier2),
        subscribe(&url3, barrier3),
    );
    fixture.stop_server().await;

    assert_eq!(
        received_events1.unwrap(),
        fixture.all_filtered_events(filter)
    );
    assert_eq!(
        received_events2.unwrap(),
        fixture.filtered_events(filter, start_index + 1)
    );
    assert_eq!(
        received_events3.unwrap(),
        fixture.filtered_events(filter, 0)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_handle_wrapping_past_max_event_id_for_main() {
    should_handle_wrapping_past_max_event_id(EventFilter::Main).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_handle_wrapping_past_max_event_id_for_signatures() {
    should_handle_wrapping_past_max_event_id(EventFilter::Signatures).await;
}

/// Rather than being a test proper, this is more a means to easily determine differences between
/// versions of the events emitted by the SSE server by comparing the contents of
/// `resources/test/sse_data_schema.json` across different versions of the codebase.
#[test]
fn schema() {
    let schema_path = format!(
        "{}/../resources/test/sse_data_schema.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let expected_schema = fs::read_to_string(schema_path).unwrap();
    let schema = schema_for!(SseData);
    assert_eq!(
        serde_json::to_string_pretty(&schema).unwrap(),
        expected_schema.trim()
    );
}
