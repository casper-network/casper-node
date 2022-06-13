//! Asynchronous multiplexing

pub mod backpressured;
pub mod chunked;
pub mod error;
pub mod fixed_size;
pub mod io;
pub mod mux;
#[cfg(test)]
pub(crate) mod pipe;

use bytes::Buf;

/// Helper macro for returning a `Poll::Ready(Err)` eagerly.
///
/// Can be remove once `Try` is stabilized for `Poll`.
#[macro_export]
macro_rules! try_ready {
    ($ex:expr) => {
        match $ex {
            Err(e) => return Poll::Ready(Err(e.into())),
            Ok(v) => v,
        }
    };
}

/// A frame for stack allocated data.
#[derive(Debug)]
pub struct ImmediateFrame<A> {
    /// How much of the frame has been read.
    pos: usize,
    /// The actual value contained.
    value: A,
}

/// Canonical encoding of immediates.
///
/// This trait describes the conversion of an immediate type from a slice of bytes.
pub trait FromFixedSize: Sized {
    /// The size of the type on the wire.
    ///
    /// `from_slice` expected its input argument to be of this length.
    const WIRE_SIZE: usize;

    /// Try to reconstruct a type from a slice of bytes.
    fn from_slice(slice: &[u8]) -> Option<Self>;
}

impl<A> ImmediateFrame<A> {
    #[inline]
    pub fn new(value: A) -> Self {
        Self { pos: 0, value }
    }
}

impl From<u8> for ImmediateFrame<[u8; 1]> {
    #[inline]
    fn from(value: u8) -> Self {
        ImmediateFrame::new(value.to_le_bytes())
    }
}

impl From<u16> for ImmediateFrame<[u8; 2]> {
    #[inline]
    fn from(value: u16) -> Self {
        ImmediateFrame::new(value.to_le_bytes())
    }
}

impl From<u32> for ImmediateFrame<[u8; 4]> {
    #[inline]
    fn from(value: u32) -> Self {
        ImmediateFrame::new(value.to_le_bytes())
    }
}

impl FromFixedSize for u8 {
    const WIRE_SIZE: usize = 1;

    fn from_slice(slice: &[u8]) -> Option<Self> {
        match *slice {
            [v] => Some(v),
            _ => None,
        }
    }
}

impl FromFixedSize for u16 {
    const WIRE_SIZE: usize = 2;

    fn from_slice(slice: &[u8]) -> Option<Self> {
        Some(u16::from_le_bytes(slice.try_into().ok()?))
    }
}

impl FromFixedSize for u32 {
    const WIRE_SIZE: usize = 4;

    fn from_slice(slice: &[u8]) -> Option<Self> {
        Some(u32::from_le_bytes(slice.try_into().ok()?))
    }
}

impl<A> Buf for ImmediateFrame<A>
where
    A: AsRef<[u8]>,
{
    fn remaining(&self) -> usize {
        // Does not overflow, as `pos` is  `< .len()`.

        self.value.as_ref().len() - self.pos
    }

    fn chunk(&self) -> &[u8] {
        // Safe access, as `pos` is guaranteed to be `< .len()`.
        &self.value.as_ref()[self.pos..]
    }

    fn advance(&mut self, cnt: usize) {
        // This is the only function modifying `pos`, upholding the invariant of it being smaller
        // than the length of the data we have.
        self.pos = (self.pos + cnt).min(self.value.as_ref().len());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        convert::Infallible,
        fmt::Debug,
        io::Read,
        num::NonZeroUsize,
        ops::Deref,
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll, Waker},
    };

    use bytes::{Buf, Bytes};
    use futures::{future, AsyncReadExt, FutureExt, Sink, SinkExt, Stream, StreamExt};
    use tokio_util::sync::PollSender;

    use crate::{
        chunked::{make_defragmentizer, make_fragmentizer},
        io::{length_delimited::LengthDelimited, FrameReader, FrameWriter},
        pipe::pipe,
    };

    // In tests use small value so that we make sure that
    // we correctly merge data that was polled from
    // the stream in small chunks.
    const BUFFER_INCREMENT: usize = 4;

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

    /// A sink for unit testing.
    ///
    /// All data sent to it will be written to a buffer immediately that can be read during
    /// operation. It is guarded by a lock so that only complete writes are visible.
    ///
    /// Additionally, a `Plug` can be inserted into the sink. While a plug is plugged in, no data
    /// can flow into the sink. In a similar manner, the sink can be clogged - while it is possible
    /// to start sending new data, it will not report being done until the clog is cleared.
    ///
    /// ```text
    ///   Item ->     (plugged?)             [             ...  ] -> (clogged?) -> done flushing
    ///    ^ Input     ^ Plug (blocks input)   ^ Buffer contents      ^ Clog, prevents flush
    /// ```
    ///
    /// This can be used to simulate a sink on a busy or slow TCP connection, for example.
    #[derive(Default, Debug)]
    pub struct TestingSink {
        /// The state of the plug.
        obstruction: Mutex<SinkObstruction>,
        /// Buffer storing all the data.
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl TestingSink {
        /// Creates a new testing sink.
        ///
        /// The sink will initially be unplugged.
        pub fn new() -> Self {
            TestingSink::default()
        }

        /// Inserts or removes the plug from the sink.
        pub fn set_plugged(&self, plugged: bool) {
            let mut guard = self.obstruction.lock().expect("could not lock plug");
            guard.plugged = plugged;

            // Notify any waiting tasks that there may be progress to be made.
            if !plugged {
                if let Some(ref waker) = guard.waker {
                    waker.wake_by_ref()
                }
            }
        }

        /// Inserts or removes the clog from the sink.
        pub fn set_clogged(&self, clogged: bool) {
            let mut guard = self.obstruction.lock().expect("could not lock plug");
            guard.clogged = clogged;

            // Notify any waiting tasks that there may be progress to be made.
            if !clogged {
                if let Some(ref waker) = guard.waker {
                    waker.wake_by_ref()
                }
            }
        }

        /// Determine whether the sink is plugged.
        ///
        /// Will update the local waker reference.
        pub fn is_plugged(&self, cx: &mut Context<'_>) -> bool {
            let mut guard = self.obstruction.lock().expect("could not lock plug");

            guard.waker = Some(cx.waker().clone());
            guard.plugged
        }

        /// Determine whether the sink is clogged.
        ///
        /// Will update the local waker reference.
        pub fn is_clogged(&self, cx: &mut Context<'_>) -> bool {
            let mut guard = self.obstruction.lock().expect("could not lock plug");

            guard.waker = Some(cx.waker().clone());
            guard.clogged
        }

        /// Returns a copy of the contents.
        pub fn get_contents(&self) -> Vec<u8> {
            Vec::clone(
                &self
                    .buffer
                    .lock()
                    .expect("could not lock test sink for copying"),
            )
        }

        /// Creates a new reference to the testing sink that also implements `Sink`.
        ///
        /// Internally, the reference has a static lifetime through `Arc` and can thus be passed
        /// on independently.
        pub fn into_ref(self: Arc<Self>) -> TestingSinkRef {
            TestingSinkRef(self)
        }

        /// Helper function for sink implementations, calling `poll_ready`.
        fn sink_poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
            if self.is_plugged(cx) {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }

        /// Helper function for sink implementations, calling `start_end`.
        fn sink_start_send<F: Buf>(&self, item: F) -> Result<(), Infallible> {
            let mut guard = self.buffer.lock().expect("could not lock buffer");

            item.reader()
                .read_to_end(&mut guard)
                .expect("writing to vec should never fail");

            Ok(())
        }

        /// Helper function for sink implementations, calling `sink_poll_flush`.
        fn sink_poll_flush(&self, cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
            // We're always done storing the data, but we pretend we need to do more if clogged.
            if self.is_clogged(cx) {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }

        /// Helper function for sink implementations, calling `sink_poll_close`.
        fn sink_poll_close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
            // Nothing to close, so this is essentially the same as flushing.
            self.sink_poll_flush(cx)
        }
    }

    /// A plug/clog inserted into the sink.
    #[derive(Debug, Default)]
    struct SinkObstruction {
        /// Whether or not the sink is plugged.
        plugged: bool,
        /// Whether or not the sink is clogged.
        clogged: bool,
        /// The waker of the last task to access the plug. Will be called when removing.
        waker: Option<Waker>,
    }

    macro_rules! sink_impl_fwd {
        ($ty:ty) => {
            impl<F: Buf> Sink<F> for $ty {
                type Error = Infallible;

                fn poll_ready(
                    self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    self.sink_poll_ready(cx)
                }

                fn start_send(self: Pin<&mut Self>, item: F) -> Result<(), Self::Error> {
                    self.sink_start_send(item)
                }

                fn poll_flush(
                    self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    self.sink_poll_flush(cx)
                }

                fn poll_close(
                    self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    self.sink_poll_close(cx)
                }
            }
        };
    }

    /// A reference to a testing sink that implements `Sink`.
    #[derive(Debug)]
    pub struct TestingSinkRef(Arc<TestingSink>);

    impl Deref for TestingSinkRef {
        type Target = TestingSink;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    sink_impl_fwd!(TestingSink);
    sink_impl_fwd!(&TestingSink);
    sink_impl_fwd!(TestingSinkRef);

    #[test]
    fn simple_lifecycle() {
        let mut sink = TestingSink::new();
        assert!(sink.send(&b"one"[..]).now_or_never().is_some());
        assert!(sink.send(&b"two"[..]).now_or_never().is_some());
        assert!(sink.send(&b"three"[..]).now_or_never().is_some());

        assert_eq!(sink.get_contents(), b"onetwothree");
    }

    #[test]
    fn plug_blocks_sink() {
        let sink = TestingSink::new();
        let mut sink_handle = &sink;

        sink.set_plugged(true);

        // The sink is plugged, so sending should fail. We also drop the future, causing the value
        // to be discarded.
        assert!(sink_handle.send(&b"dummy"[..]).now_or_never().is_none());
        assert!(sink.get_contents().is_empty());

        // Now stuff more data into the sink.
        let second_send = sink_handle.send(&b"second"[..]);
        sink.set_plugged(false);
        assert!(second_send.now_or_never().is_some());
        assert!(sink_handle.send(&b"third"[..]).now_or_never().is_some());
        assert_eq!(sink.get_contents(), b"secondthird");
    }

    #[test]
    fn clog_blocks_sink_completion() {
        let sink = TestingSink::new();
        let mut sink_handle = &sink;

        sink.set_clogged(true);

        // The sink is clogged, so sending should fail to complete, but it is written.
        assert!(sink_handle.send(&b"first"[..]).now_or_never().is_none());
        assert_eq!(sink.get_contents(), b"first");

        // Now stuff more data into the sink.
        let second_send = sink_handle.send(&b"second"[..]);
        sink.set_clogged(false);
        assert!(second_send.now_or_never().is_some());
        assert!(sink_handle.send(&b"third"[..]).now_or_never().is_some());
        assert_eq!(sink.get_contents(), b"firstsecondthird");
    }

    #[tokio::test]
    async fn ensure_sink_wakes_up_after_plugging_in() {
        let sink = Arc::new(TestingSink::new());

        sink.set_plugged(true);

        let sink_alt = sink.clone();

        let join_handle = tokio::spawn(async move {
            sink_alt.as_ref().send(&b"sample"[..]).await.unwrap();
        });

        tokio::task::yield_now().await;
        sink.set_plugged(false);

        // This will block forever if the other task is not woken up. To verify, comment out the
        // `Waker::wake_by_ref` call in the sink implementation.
        join_handle.await.unwrap();
    }

    /// Test an "end-to-end" instance of the assembled pipeline for sending.
    #[test]
    fn chunked_length_prefixed_sink() {
        let (tx, rx) = pipe();

        let frame_writer = FrameWriter::new(LengthDelimited, tx);
        let mut chunked_sink =
            make_fragmentizer::<_, Infallible>(frame_writer, NonZeroUsize::new(5).unwrap());

        let frame_reader = FrameReader::new(LengthDelimited, rx, BUFFER_INCREMENT);
        let chunked_reader = make_defragmentizer(frame_reader);

        let sample_data = Bytes::from(&b"QRSTUV"[..]);

        chunked_sink
            .send(sample_data)
            .now_or_never()
            .unwrap()
            .expect("send failed");

        // Drop the sink, to ensure it is closed.
        drop(chunked_sink);

        let round_tripped: Vec<_> = chunked_reader.collect().now_or_never().unwrap();

        assert_eq!(round_tripped, &[&b"QRSTUV"[..]])
    }

    #[test]
    fn from_bytestream_to_frame() {
        let input = &b"\x06\x00\x00ABCDE\x06\x00\x00FGHIJ\x03\x00\xffKL"[..];
        let expected = "ABCDEFGHIJKL";

        let defragmentizer =
            make_defragmentizer(FrameReader::new(LengthDelimited, input, BUFFER_INCREMENT));

        let messages: Vec<_> = defragmentizer.collect().now_or_never().unwrap();
        assert_eq!(
            expected,
            messages.first().expect("should have at least one message")
        );
    }

    #[test]
    fn from_bytestream_to_multiple_frames() {
        let input = &b"\x06\x00\x00ABCDE\x06\x00\x00FGHIJ\x03\x00\xffKL\x0d\x00\xffSINGLE_CHUNK\x02\x00\x00C\x02\x00\x00R\x02\x00\x00U\x02\x00\x00M\x02\x00\x00B\x02\x00\xffS"[..];
        let expected: &[&[u8]] = &[b"ABCDEFGHIJKL", b"SINGLE_CHUNK", b"CRUMBS"];

        let defragmentizer =
            make_defragmentizer(FrameReader::new(LengthDelimited, input, BUFFER_INCREMENT));

        let messages: Vec<_> = defragmentizer.collect().now_or_never().unwrap();
        assert_eq!(expected, messages);
    }
}
