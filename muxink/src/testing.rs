//! Testing support utilities.

pub mod pipe;
pub mod testing_sink;

use std::{fmt::Debug, io::Read};

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
