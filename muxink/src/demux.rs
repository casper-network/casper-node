//! Stream demultiplexing
//!
//! Demultiplexes a Stream of Bytes into multiple channels. Up to 256 channels are supported, and
//! if messages are present on a channel but there isn't an associated DemultiplexerHandle for that
//! channel, then the Stream will never poll as Ready.

use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{stream::Fuse, Stream, StreamExt};

/// A frame demultiplexer.
///
/// A demultiplexer is not used directly, but used to spawn demultiplexing handles.
///
/// TODO What if someone sends data to a channel for which there is no handle?
///      I can think of two reasonable responses:
///         1. return an error to the handle which saw this message.
///         2. drop all messages we receive which don't have a corresponding `DemultiplexerHandle`
///            yet.
///         3. allow messages to sit forever and block the rest of the handles, preferring whoever
///            is sending us the messages to filter out ones which aren't for a channel we're
///            listening on. this is already what happens if a `DemultiplexerHandle` for any
///            channel which has messages in the stream doesn't ever take them out.
pub struct Demultiplexer<S> {
    stream: Fuse<S>,
    next_frame: Option<(u8, Bytes)>,
}

impl<S: Stream> Demultiplexer<S> {
    /// Creates a new demultiplexer with the given underlying stream.
    pub fn new(stream: S) -> Demultiplexer<S> {
        Demultiplexer {
            stream: stream.fuse(),
            next_frame: None,
        }
    }
}

impl<S> Demultiplexer<S> {
    /// Creates a handle listening for frames on the given channel.
    ///
    /// Any item on this channel sent to the `Stream` underlying the `Demultiplexer` we used to
    /// create this handle will be read only when all other messages for other channels have been
    /// read first. If one has handles on the same channel created via the same underlying
    /// `Demultiplexer`, each message on that channel will only be received by one of the handles.
    /// Unless this is desired behavior, this should be avoided.
    pub fn create_handle(demux: Arc<Mutex<Self>>, channel: u8) -> DemultiplexerHandle<S> {
        DemultiplexerHandle {
            channel,
            demux: demux.clone(),
        }
    }
}

/// A handle to a demultiplexer.
///
/// A handle is bound to a specific channel, see [`Demultiplexer::create_handle`] for details.
pub struct DemultiplexerHandle<S> {
    /// Which channel this handle is listening on
    channel: u8,
    /// A reference to the underlying demultiplexer.
    demux: Arc<Mutex<Demultiplexer<S>>>, // (probably?) make sure this is a stdmutex
}

impl<S> Stream for DemultiplexerHandle<S>
where
    S: Stream<Item = Bytes> + Unpin,
{
    // TODO Result<Bytes, Error>
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Lock the demultiplexer
        let mut demux = match self.demux.as_ref().try_lock() {
            Err(_err) => panic!("TODO"), // TODO return Err("TODO")
            Ok(guard) => guard,
        };

        // If next_frame has a suitable frame for this channel, return it in a Poll::Ready. If it has
        // an unsuitable frame, return Poll::Pending. Otherwise, we attempt to read from the stream.
        if let Some((ref channel, ref bytes)) = demux.next_frame {
            if *channel == self.channel {
                let bytes = bytes.clone();
                demux.next_frame = None;
                return Poll::Ready(Some(bytes));
            } else {
                return Poll::Pending;
            }
        }

        // Try to read from the stream, placing the frame into next_frame and returning
        // Poll::Pending if its in the wrong channel, otherwise returning it in a Poll::Ready.
        match demux.stream.poll_next_unpin(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(bytes)) => {
                let channel: u8 = *&bytes[0..1][0];
                let frame = bytes.slice(1..).clone();
                if channel == self.channel {
                    Poll::Ready(Some(frame))
                } else {
                    demux.next_frame = Some((channel, frame));
                    Poll::Pending
                }
            }
            Poll::Ready(None) => Poll::Ready(None),
        }

        // TODO: figure out when we are being polled again, does it work correctly (see waker) or
        //       will it cause inefficient races? do we need to call wake? probably. (possibly
        //       necessary) can have table of wakers to only wake the right one.
    }
}

#[cfg(test)]
mod tests {
    use std::marker::Unpin;

    use super::*;
    use futures::{FutureExt, Stream, StreamExt};

    // This stream is used because it is not safe to call it after it returns
    // [`Poll::Ready(None)`], whereas many other streams are. The interface for
    // streams says that in general it is not safe, so it is important to test
    // using a stream which has this property as well.
    struct TestStream<T> {
        // The items which will be returned by the stream in reverse order
        items: Vec<T>,
        // Once this is set to true, this `Stream` will panic upon calling [`Stream::poll_next`]
        finished: bool,
    }

    impl<T> TestStream<T> {
        fn new(mut items: Vec<T>) -> Self {
            // We reverse the items as we use the pop method to remove them one by one,
            // thus they come out in the order specified by the `Vec`.
            items.reverse();
            TestStream {
                items,
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
            if let Some(t) = self.items.pop() {
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

    #[test]
    fn demultiplexing_two_channels() {
        // We demultiplex two channels, 0 and 1
        let items = vec![
            Bytes::copy_from_slice(&[0, 1, 2, 3, 4]),
            Bytes::copy_from_slice(&[0, 4]),
            Bytes::copy_from_slice(&[1, 2]),
            Bytes::copy_from_slice(&[1, 5]),
        ];
        let stream = TestStream::new(items);
        let demux = Arc::new(Mutex::new(Demultiplexer::new(stream)));

        // We make two handles, one for the 0 channel and another for the 1 channel
        let mut zero_handle = Demultiplexer::create_handle(demux.clone(), 0);
        let mut one_handle = Demultiplexer::create_handle(demux.clone(), 1);

        // We know the order that these things have to be awaited, so we can make sure that exactly
        // what we expects happens using the `now_or_never` function.

        // First, we expect the zero channel to have a frame.
        assert_eq!(
            zero_handle
                .next()
                .now_or_never()
                .expect("not ready")
                .expect("stream ended")
                .as_ref(),
            &[1, 2, 3, 4]
        );

        // Next, we expect that the one handle will not have a frame, but it will read off the
        // frame ready for the zero value and put it in the next_frame slot.
        assert!(one_handle.next().now_or_never().is_none());

        // It should be safe to call this again, though this time it won't even check the stream
        // and will simply notice that the next_frame slot is filled with a frame for a channel
        // which isn't 1.
        assert!(one_handle.next().now_or_never().is_none());

        // Then, we receive the message from the zero handle which the one handle left for us.
        assert_eq!(
            zero_handle
                .next()
                .now_or_never()
                .expect("not ready")
                .expect("stream ended")
                .as_ref(),
            &[4]
        );

        // Then, we pull out the message for the one handle, which hasn't yet been put on the
        // stream.
        assert_eq!(
            one_handle
                .next()
                .now_or_never()
                .expect("not ready")
                .expect("stream ended")
                .as_ref(),
            &[2]
        );

        // Now, we try to pull out a zero message again, filling the next_frame slot for the one
        // handle.
        assert!(zero_handle.next().now_or_never().is_none());

        // We take off the final value from the next_frame slot
        assert_eq!(
            one_handle
                .next()
                .now_or_never()
                .expect("not ready")
                .expect("stream ended")
                .as_ref(),
            &[5]
        );

        // Now, we assert that its safe to call this again with both the one and zero handle,
        // ensuring that the [`Fuse<S>`] truly did fuse away the danger from our dangerous
        // `TestStream`.
        assert!(one_handle.next().now_or_never().unwrap().is_none());
        assert!(zero_handle.next().now_or_never().unwrap().is_none());
    }
}
