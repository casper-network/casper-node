use std::{convert::Infallible, sync::Arc};

use bytes::Bytes;
use futures::{Sink, SinkExt, Stream, StreamExt};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::PollSender;

use crate::{
    backpressured::{BackpressuredSink, BackpressuredStream},
    testing::testing_sink::{TestingSink, TestingSinkRef},
};

/// Window size used in tests.
pub const WINDOW_SIZE: u64 = 3;

/// Sets up a `Sink`/`Stream` pair that outputs infallible results.
pub fn setup_io_pipe<T: Send + Sync + 'static>(
    size: usize,
) -> (
    impl Sink<T, Error = Infallible> + Unpin + 'static,
    impl Stream<Item = Result<T, Infallible>> + Unpin + 'static,
) {
    let (send, recv) = tokio::sync::mpsc::channel::<T>(size);

    let stream = ReceiverStream::new(recv).map(Ok);

    let sink =
        PollSender::new(send).sink_map_err(|_err| panic!("did not expect a `PollSendError`"));

    (sink, stream)
}

/// A common set of fixtures used in the backpressure tests.
///
/// The fixtures represent what a server holds when dealing with a backpressured client.
pub struct OneWayFixtures {
    /// A sender for ACKs back to the client.
    pub ack_sink: Box<dyn Sink<u64, Error = Infallible> + Unpin>,
    /// The clients sink for requests, with no backpressure wrapper. Used for retrieving the
    /// test data in the end or setting plugged/clogged status.
    pub sink: Arc<TestingSink>,
    /// The properly set up backpressured sink.
    pub bp: BackpressuredSink<
        TestingSinkRef,
        Box<dyn Stream<Item = Result<u64, Infallible>> + Unpin>,
        Bytes,
    >,
}

impl OneWayFixtures {
    /// Creates a new set of fixtures.
    pub fn new() -> Self {
        let sink = Arc::new(TestingSink::new());

        let (raw_ack_sink, raw_ack_stream) = setup_io_pipe::<u64>(1024);

        // The ACK stream and sink need to be boxed to make their types named.
        let ack_sink: Box<dyn Sink<u64, Error = Infallible> + Unpin> = Box::new(raw_ack_sink);
        let ack_stream: Box<dyn Stream<Item = Result<u64, Infallible>> + Unpin> =
            Box::new(raw_ack_stream);

        let bp = BackpressuredSink::new(sink.clone().into_ref(), ack_stream, WINDOW_SIZE);

        Self { ack_sink, sink, bp }
    }
}

impl Default for OneWayFixtures {
    fn default() -> Self {
        Self::new()
    }
}

/// A more complicated setup for testing backpressure that allows accessing both sides of the
/// connection.
///
/// The resulting `client` sends byte frames across to the `server`, with ACKs flowing through
/// the associated ACK pipe.
#[allow(clippy::type_complexity)]
pub struct TwoWayFixtures {
    pub client: BackpressuredSink<
        Box<dyn Sink<Bytes, Error = Infallible> + Send + Unpin>,
        Box<dyn Stream<Item = Result<u64, Infallible>> + Send + Unpin>,
        Bytes,
    >,
    pub server: BackpressuredStream<
        Box<dyn Stream<Item = Result<Bytes, Infallible>> + Send + Unpin>,
        Box<dyn Sink<u64, Error = Infallible> + Send + Unpin>,
        Bytes,
    >,
}

impl TwoWayFixtures {
    /// Creates a new set of two-way fixtures.
    pub fn new(size: usize) -> Self {
        Self::new_with_window(size, WINDOW_SIZE)
    }
    /// Creates a new set of two-way fixtures with a specified window size.
    pub fn new_with_window(size: usize, window_size: u64) -> Self {
        let (sink, stream) = setup_io_pipe::<Bytes>(size);

        let (ack_sink, ack_stream) = setup_io_pipe::<u64>(size);

        let boxed_sink: Box<dyn Sink<Bytes, Error = Infallible> + Send + Unpin + 'static> =
            Box::new(sink);
        let boxed_ack_stream: Box<dyn Stream<Item = Result<u64, Infallible>> + Send + Unpin> =
            Box::new(ack_stream);

        let client = BackpressuredSink::new(boxed_sink, boxed_ack_stream, window_size);

        let boxed_stream: Box<dyn Stream<Item = Result<Bytes, Infallible>> + Send + Unpin> =
            Box::new(stream);
        let boxed_ack_sink: Box<dyn Sink<u64, Error = Infallible> + Send + Unpin> =
            Box::new(ack_sink);
        let server = BackpressuredStream::new(boxed_stream, boxed_ack_sink, window_size);

        TwoWayFixtures { client, server }
    }
}
