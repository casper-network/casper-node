//! Testing support utilities.

pub mod encoding;
pub mod pipe;
pub mod testing_sink;

use std::{
    collections::VecDeque,
    fmt::Debug,
    io::Read,
    marker::Unpin,
    pin::Pin,
    result::Result,
    task::{Context, Poll},
};

use bytes::Buf;
use futures::{FutureExt, Stream, StreamExt};

// In tests use small value to make sure that we correctly merge data that was polled from the
// stream in small fragments.
pub const TESTING_BUFFER_INCREMENT: usize = 4;

/// Collects everything inside a `Buf` into a `Vec`.
pub fn collect_buf<B: Buf>(buf: B) -> Vec<u8> {
    let mut vec = Vec::new();
    buf.reader()
        .read_to_end(&mut vec)
        .expect("reading buf should never fail");
    vec
}

/// Collects the contents of multiple `Buf`s into a single flattened `Vec`.
pub fn collect_bufs<B: Buf, I: IntoIterator<Item = B>>(items: I) -> Vec<u8> {
    let mut vec = Vec::new();
    for buf in items.into_iter() {
        buf.reader()
            .read_to_end(&mut vec)
            .expect("reading buf should never fail");
    }
    vec
}

/// Given a stream producing results, returns the values.
///
/// # Panics
///
/// Panics if the future is not `Poll::Ready` or any value is an error.
pub fn collect_stream_results<T, E, S>(stream: S) -> Vec<T>
where
    E: Debug,
    S: Stream<Item = Result<T, E>>,
{
    let results: Vec<_> = stream.collect().now_or_never().expect("stream not ready");
    results
        .into_iter()
        .collect::<Result<_, _>>()
        .expect("error in stream results")
}

// This stream is used because it is not safe to call it after it returns
// [`Poll::Ready(None)`], whereas many other streams are. The interface for
// streams says that in general it is not safe, so it is important to test
// using a stream which has this property as well.
pub(crate) struct TestStream<T> {
    // The items which will be returned by the stream in reverse order
    items: VecDeque<T>,
    // Once this is set to true, this `Stream` will panic upon calling [`Stream::poll_next`]
    finished: bool,
}

impl<T> TestStream<T> {
    pub(crate) fn new(items: Vec<T>) -> Self {
        TestStream {
            items: items.into(),
            finished: false,
        }
    }
}

// We implement Unpin because of the constraint in the implementation of the
// `DemultiplexerHandle`.
impl<T> Unpin for TestStream<T> {}

impl<T> Stream for TestStream<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Panic if we've already emitted [`Poll::Ready(None)`]
        if self.finished {
            panic!("polled a TestStream after completion");
        }
        if let Some(t) = self.items.pop_front() {
            return Poll::Ready(Some(t));
        } else {
            // Before we return None, make sure we set finished to true so that calling this
            // again will result in a panic, as the specification for `Stream` tells us is
            // possible with an arbitrary implementation.
            self.finished = true;
            return Poll::Ready(None);
        }
    }
}
