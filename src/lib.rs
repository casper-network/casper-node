pub mod backpressured;
pub mod chunked;
pub mod error;
pub mod length_prefixed;
pub mod mux;

use bytes::Buf;

/// A frame for stack allocated data.
#[derive(Debug)]
pub struct ImmediateFrame<A> {
    /// How much of the frame has been read.
    pos: usize,
    /// The actual value contained.
    value: A,
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
        io::Read,
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll, Waker},
    };

    use bytes::{Buf, Bytes};
    use futures::{future, stream, FutureExt, Sink, SinkExt};

    use crate::{
        chunked::{chunk_frame, SingleChunk},
        error::Error,
        length_prefixed::{frame_add_length_prefix, LengthPrefixedFrame},
    };

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

    /// A sink for unit testing.
    ///
    /// All data sent to it will be written to a buffer immediately that can be read during
    /// operation. It is guarded by a lock so that only complete writes are visible.
    ///
    /// Additionally, a `Plug` can be inserted into the sink. While a plug is plugged in, no data
    /// can flow into the sink.
    #[derive(Debug)]
    struct TestingSink {
        /// The engagement of the plug.
        plug: Mutex<Plug>,
        /// Buffer storing all the data.
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl TestingSink {
        /// Inserts or removes the plug from the sink.
        pub fn set_plugged(&self, plugged: bool) {
            let mut guard = self.plug.lock().expect("could not lock plug");
            guard.plugged = plugged;

            // Notify any waiting tasks that there may be progress to be made.
            if !plugged {
                // TODO: Write test that should fail because this line is absent first.
                // guard.waker.wake_by_ref()
            }
        }

        /// Determine whether the sink is plugged.
        ///
        /// Will update the local waker reference.
        fn is_plugged(&self, cx: &mut Context<'_>) -> bool {
            let mut guard = self.plug.lock().expect("could not lock plug");

            // Register waker.
            guard.waker = cx.waker().clone();
            guard.plugged
        }
    }

    /// A plug inserted into the sink.
    #[derive(Debug)]
    struct Plug {
        /// Whether or not the plug is engaged.
        plugged: bool,
        /// The waker of the last task to access the plug. Will be called when unplugging.
        waker: Waker,
    }

    impl<F: Buf> Sink<F> for &TestingSink {
        type Error = Infallible;

        fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            if self.is_plugged(cx) {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }

        fn start_send(self: Pin<&mut Self>, item: F) -> Result<(), Self::Error> {
            let mut guard = self.buffer.lock().expect("could not lock buffer");

            item.reader()
                .read_to_end(&mut guard)
                .expect("writing to vec should never fail");

            Ok(())
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            // We're always done flushing, since we write the entire item when sending. Still, we
            // use this as an opportunity to plug if necessary.
            if self.is_plugged(cx) {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }

        fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            // Nothing to close, so this is essentially the same as flushing.
            Sink::<F>::poll_flush(self, cx)
        }
    }

    /// Test an "end-to-end" instance of the assembled pipeline for sending.
    #[test]
    fn chunked_length_prefixed_sink() {
        let base_sink: Vec<LengthPrefixedFrame<SingleChunk>> = Vec::new();

        let length_prefixed_sink =
            base_sink.with(|frame| future::ready(frame_add_length_prefix(frame)));

        let mut chunked_sink = length_prefixed_sink.with_flat_map(|frame| {
            let chunk_iter = chunk_frame(frame, 5.try_into().unwrap()).expect("TODO: Handle error");
            stream::iter(chunk_iter.map(Result::<_, Error>::Ok))
        });

        let sample_data = Bytes::from(&b"QRSTUV"[..]);

        chunked_sink
            .send(sample_data)
            .now_or_never()
            .unwrap()
            .expect("send failed");

        let chunks: Vec<_> = chunked_sink
            .into_inner()
            .into_inner()
            .into_iter()
            .map(collect_buf)
            .collect();

        assert_eq!(
            chunks,
            vec![b"\x06\x00\x00QRSTU".to_vec(), b"\x02\x00\xffV".to_vec()]
        )
    }
}
