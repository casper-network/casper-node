//! Stream demultiplexing
//!
//! Demultiplexes a Stream of Bytes into multiple channels. Up to 256 channels are supported, and
//! if messages are present on a channel but there isn't an associated DemultiplexerHandle for that
//! channel, then the Stream will never poll as Ready.

use std::{
    error::Error,
    pin::Pin,
    result::Result,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use bytes::{Buf, Bytes};
use futures::{ready, stream::Fuse, Stream, StreamExt};
use thiserror::Error as ThisError;

const CHANNEL_BYTE_COUNT: usize = MAX_CHANNELS / CHANNELS_PER_BYTE;
const CHANNEL_BYTE_SHIFT: usize = 3;
const CHANNELS_PER_BYTE: usize = 8;
const MAX_CHANNELS: usize = 256;

#[derive(Debug, ThisError)]
pub enum DemultiplexerError<E> {
    #[error("Channel {0} is already in use")]
    ChannelUnavailable(u8),
    #[error("Received a message of length 0")]
    EmptyMessage,
    #[error("Message on channel {0} has no frame")]
    MissingFrame(u8),
    #[error("Stream error: {0}")]
    Stream(E),
}

/// A frame demultiplexer.
///
/// A demultiplexer is not used directly, but used to spawn demultiplexing handles.
pub struct Demultiplexer<S> {
    /// The underlying `Stream`, `Fuse`d in order to make it safe to be called once its output
    /// `Poll::Ready(None)`.
    stream: Fuse<S>,
    /// Holds the frame and channel, if available, which has been read by a `DemultiplexerHandle`
    /// corresponding to a different channel.
    next_frame: Option<(u8, Bytes)>,
    /// A bit-field representing the channels which have had `DemultiplexerHandle`s constructed.
    active_channels: [u8; CHANNEL_BYTE_COUNT],
}

impl<S: Stream> Demultiplexer<S> {
    /// Creates a new demultiplexer with the given underlying stream.
    pub fn new(stream: S) -> Demultiplexer<S> {
        Demultiplexer {
            // We fuse the stream in case its unsafe to call it after yielding `Poll::Ready(None)`
            stream: stream.fuse(),
            // Initially, we have no next frame
            next_frame: None,
            // Initially, all channels are inactive
            active_channels: [0b00000000; CHANNEL_BYTE_COUNT],
        }
    }
}

// Here, we write the logic for accessing and modifying the bit-field representing the active
// channels.
impl<S> Demultiplexer<S> {
    fn activate_channel(&mut self, channel: u8) {
        self.active_channels[(channel >> CHANNEL_BYTE_SHIFT) as usize] |=
            1 << (channel & (CHANNELS_PER_BYTE as u8 - 1));
    }

    fn deactivate_channel(&mut self, channel: u8) {
        self.active_channels[(channel >> CHANNEL_BYTE_SHIFT) as usize] &=
            !(1 << (channel & (CHANNELS_PER_BYTE as u8 - 1)));
    }

    fn channel_is_active(&self, channel: u8) -> bool {
        (self.active_channels[(channel >> CHANNEL_BYTE_SHIFT) as usize]
            & (1 << (channel & (CHANNELS_PER_BYTE as u8 - 1))))
            != 0
    }
}

impl<S: Stream> Demultiplexer<S> {
    /// Creates a handle listening for frames on the given channel.
    ///
    /// Items received through a given handle may be blocked if other handles on the same
    /// Demultiplexer are not polled at the same time. If one has handles on the same
    /// channel created via the same underlying `Demultiplexer`, each message on that channel
    /// will only be received by one of the handles.
    pub fn create_handle<E: Error>(
        demux: Arc<Mutex<Self>>,
        channel: u8,
    ) -> Result<DemultiplexerHandle<S>, DemultiplexerError<E>> {
        let mut demux_guard = demux.lock().expect("poisoned lock");

        if demux_guard.channel_is_active(channel) {
            return Err(DemultiplexerError::ChannelUnavailable(channel));
        }

        demux_guard.activate_channel(channel);

        Ok(DemultiplexerHandle {
            channel,
            demux: demux.clone(),
        })
    }
}

/// A handle to a demultiplexer.
///
/// A handle is bound to a specific channel, see [`Demultiplexer::create_handle`] for details.
pub struct DemultiplexerHandle<S> {
    /// Which channel this handle is listening on.
    channel: u8,
    /// A reference to the underlying demultiplexer.
    demux: Arc<Mutex<Demultiplexer<S>>>,
}

impl<S> Drop for DemultiplexerHandle<S> {
    fn drop(&mut self) {
        self.demux
            .lock()
            .expect("poisoned lock")
            .deactivate_channel(self.channel);
    }
}

impl<S, E> Stream for DemultiplexerHandle<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: Error,
{
    type Item = Result<Bytes, DemultiplexerError<E>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Lock the demultiplexer.
        let mut demux = self.demux.lock().expect("poisoned lock");

        // If next_frame has a suitable frame for this channel, return it in a `Poll::Ready`. If it
        // has an unsuitable frame, return `Poll::Pending`. Otherwise, we attempt to read
        // from the stream.
        if let Some((ref channel, ref bytes)) = demux.next_frame {
            if *channel == self.channel {
                let bytes = bytes.clone();
                demux.next_frame = None;
                return Poll::Ready(Some(Ok(bytes)));
            } else {
                return Poll::Pending;
            }
        }

        // Try to read from the stream, placing the frame into `next_frame` and returning
        // `Poll::Pending` if it's in the wrong channel, otherwise returning it in a `Poll::Ready`.
        match ready!(demux.stream.poll_next_unpin(cx)) {
            Some(Ok(mut bytes)) => {
                if bytes.is_empty() {
                    return Poll::Ready(Some(Err(DemultiplexerError::EmptyMessage)));
                }

                let channel = bytes.get_u8();
                if bytes.is_empty() {
                    return Poll::Ready(Some(Err(DemultiplexerError::MissingFrame(channel))));
                }

                if channel == self.channel {
                    Poll::Ready(Some(Ok(bytes)))
                } else {
                    demux.next_frame = Some((channel, bytes));
                    Poll::Pending
                }
            }
            Some(Err(err)) => return Poll::Ready(Some(Err(DemultiplexerError::Stream(err)))),
            None => Poll::Ready(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Error as IoError, marker::Unpin};

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
    fn channel_activation() {
        let items: Vec<Result<Bytes, DemultiplexerError<IoError>>> = vec![];
        let stream = TestStream::new(items);
        let mut demux = Demultiplexer::new(stream);

        let examples: Vec<u8> = (0u8..255u8).collect();

        for i in examples.iter().copied() {
            assert!(!demux.channel_is_active(i));
            demux.activate_channel(i);
            assert!(demux.channel_is_active(i));
        }

        for i in examples.iter().copied() {
            demux.deactivate_channel(i);
            assert!(!demux.channel_is_active(i));
        }
    }

    #[test]
    fn demultiplexing_two_channels() {
        // We demultiplex two channels, 0 and 1
        let items: Vec<Result<Bytes, DemultiplexerError<IoError>>> = [
            Bytes::copy_from_slice(&[0, 1, 2, 3, 4]),
            Bytes::copy_from_slice(&[0, 4]),
            Bytes::copy_from_slice(&[1, 2]),
            Bytes::copy_from_slice(&[1, 5]),
        ]
        .into_iter()
        .map(Result::Ok)
        .collect();
        let stream = TestStream::new(items);
        let demux = Arc::new(Mutex::new(Demultiplexer::new(stream)));

        // We make two handles, one for the 0 channel and another for the 1 channel
        let mut zero_handle = Demultiplexer::create_handle::<IoError>(demux.clone(), 0).unwrap();
        let mut one_handle = Demultiplexer::create_handle::<IoError>(demux.clone(), 1).unwrap();

        // We know the order that these things have to be awaited, so we can make sure that exactly
        // what we expects happens using the `now_or_never` function.

        // First, we expect the zero channel to have a frame.
        assert_eq!(
            zero_handle
                .next()
                .now_or_never()
                .expect("not ready")
                .expect("stream ended")
                .expect("item is error")
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
                .expect("item is error")
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
                .expect("item is error")
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
                .expect("item is error")
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
