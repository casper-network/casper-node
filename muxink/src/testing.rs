//! Testing support utilities.

pub mod encoding;
pub mod fixtures;
pub mod pipe;
pub mod testing_sink;
pub mod testing_stream;

use std::{
    fmt::Debug,
    io::Read,
    result::Result,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use bytes::Buf;
use futures::{Future, FutureExt, Stream, StreamExt};
use tokio::task::JoinHandle;

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

/// A background task that can be asked whether it has completed or not.
#[derive(Debug)]
pub(crate) struct BackgroundTask<T> {
    /// Join handle for the background task.
    join_handle: JoinHandle<T>,
    /// Indicates the task has started.
    started: Arc<AtomicBool>,
    /// Indicates the task has finished.
    ended: Arc<AtomicBool>,
}

impl<T> BackgroundTask<T>
where
    T: Send,
{
    /// Spawns a new background task.
    pub(crate) fn spawn<F>(fut: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
        T: 'static,
    {
        let started = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));

        let (s, e) = (started.clone(), ended.clone());
        let join_handle = tokio::spawn(async move {
            s.store(true, Ordering::SeqCst);
            let rv = fut.await;
            e.store(true, Ordering::SeqCst);

            rv
        });

        BackgroundTask {
            join_handle,
            started,
            ended,
        }
    }

    /// Returns whether or not the task has finished.
    pub(crate) fn has_finished(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    /// Returns whether or not the task has begun.
    pub(crate) fn has_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    /// Returns whether or not the task is currently executing.
    pub(crate) fn is_running(&self) -> bool {
        self.has_started() && !self.has_finished()
    }

    /// Waits for the task to complete and returns its output.
    pub(crate) async fn retrieve_output(self) -> T {
        self.join_handle.await.expect("future has panicked")
    }
}
