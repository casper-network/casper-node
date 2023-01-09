/// Generic testing stream.
use std::{
    collections::VecDeque,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Duration,
};

use futures::{FutureExt, Stream, StreamExt};

/// A testing stream that returns predetermined items.
///
/// Returns [`Poll::Ready(None)`] only once, subsequent polling after it has finished will result
/// in a panic.
///
/// Can be paused via [`StreamControl::pause`].
#[derive(Debug)]
pub(crate) struct TestingStream<T> {
    /// The items to be returned by the stream.
    items: VecDeque<T>,
    /// Indicates the stream has finished, causing subsequent polls to panic.
    finished: bool,
    /// Control object for stream.
    control: Arc<Mutex<StreamControl>>,
}

/// A reference to a testing stream.
#[derive(Debug)]
pub(crate) struct StreamControlRef(Arc<Mutex<StreamControl>>);

/// Stream control for pausing and unpausing.
#[derive(Debug, Default)]
pub(crate) struct StreamControl {
    /// Whether the stream should return [`Poll::Pending`] at the moment.
    paused: bool,
    /// The waker to reawake the stream after unpausing.
    waker: Option<Waker>,
}

impl StreamControlRef {
    /// Pauses the stream.
    ///
    /// Subsequent polling of the stream will result in `Pending` being returned.
    pub(crate) fn pause(&self) {
        let mut guard = self.0.lock().expect("stream control poisoned");
        guard.paused = true;
    }

    /// Unpauses the stream.
    ///
    /// Causes the stream to resume. If it was paused, any waiting tasks will be woken up.
    pub(crate) fn unpause(&self) {
        let mut guard = self.0.lock().expect("stream control poisoned");

        if let Some(waker) = guard.waker.take() {
            waker.wake();
        }
        guard.paused = false;
    }
}

impl<T> TestingStream<T> {
    /// Creates a new stream for testing.
    pub(crate) fn new<I: IntoIterator<Item = T>>(items: I) -> Self {
        TestingStream {
            items: items.into_iter().collect(),
            finished: false,
            control: Default::default(),
        }
    }

    /// Creates a new reference to the testing stream controls.
    pub(crate) fn control(&self) -> StreamControlRef {
        StreamControlRef(self.control.clone())
    }
}

// We implement Unpin because of the constraint in the implementation of the
// `DemultiplexerHandle`.
// TODO: Remove this.
impl<T> Unpin for TestingStream<T> {}

impl<T> Stream for TestingStream<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        {
            let mut guard = self.control.lock().expect("stream control poisoned");

            if guard.paused {
                guard.waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
        }

        // Panic if we've already emitted [`Poll::Ready(None)`]
        if self.finished {
            panic!("polled a TestStream after completion");
        }
        if let Some(t) = self.items.pop_front() {
            Poll::Ready(Some(t))
        } else {
            // Before we return None, make sure we set finished to true so that calling this
            // again will result in a panic, as the specification for `Stream` tells us is
            // possible with an arbitrary implementation.
            self.finished = true;
            Poll::Ready(None)
        }
    }
}

#[tokio::test]
async fn smoke_test() {
    let mut stream = TestingStream::new([1, 2, 3]);

    assert_eq!(stream.next().await, Some(1));
    assert_eq!(stream.next().await, Some(2));
    assert_eq!(stream.next().await, Some(3));
    assert_eq!(stream.next().await, None);
}

#[tokio::test]
#[should_panic(expected = "polled a TestStream after completion")]
async fn stream_panics_if_polled_after_ready() {
    let mut stream = TestingStream::new([1, 2, 3]);
    stream.next().await;
    stream.next().await;
    stream.next().await;
    stream.next().await;
    stream.next().await;
}

#[test]
fn stream_can_be_paused() {
    let mut stream = TestingStream::new([1, 2, 3]);

    assert_eq!(
        stream.next().now_or_never().expect("should be ready"),
        Some(1)
    );

    stream.control().pause();
    assert!(stream.next().now_or_never().is_none());
    assert!(stream.next().now_or_never().is_none());
    stream.control().unpause();

    assert_eq!(
        stream.next().now_or_never().expect("should be ready"),
        Some(2)
    );
}

#[tokio::test]
async fn stream_unpausing_wakes_up_test_stream() {
    let mut stream = TestingStream::new([1, 2, 3]);
    let ctrl = stream.control();
    ctrl.pause();

    let reader = tokio::spawn(async move {
        stream.next().await;
        stream.next().await;
        stream.next().await;
        assert!(stream.next().await.is_none());
    });

    // Allow for a little bit of time for the reader to block.
    tokio::time::sleep(Duration::from_millis(50)).await;

    ctrl.unpause();

    // After unpausing, the reader should be able to finish.
    tokio::time::timeout(Duration::from_secs(1), reader)
        .await
        .expect("should not timeout")
        .expect("should join successfully");
}
