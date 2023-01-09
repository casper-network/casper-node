//! Stream demultiplexing
//!
//! Demultiplexes a Stream of Bytes into multiple channels. Up to 256 channels are supported, and if
//! messages are present on a channel but there isn't an associated [`DemultiplexerHandle`] for that
//! channel, then the stream will never poll as ready.

use std::{
    error::Error,
    pin::Pin,
    result::Result,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use bytes::{Buf, Bytes};
use futures::{ready, Stream, StreamExt};
use thiserror::Error as ThisError;

const CHANNEL_BYTE_COUNT: usize = MAX_CHANNELS / CHANNELS_PER_BYTE;
const CHANNEL_BYTE_SHIFT: usize = 3;
const CHANNELS_PER_BYTE: usize = 8;
const MAX_CHANNELS: usize = 256;

#[derive(Debug, ThisError)]
pub enum DemultiplexerError<E> {
    #[error("Received message on channel {0} but no handle is listening")]
    ChannelNotActive(u8),
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
    /// The underlying `Stream`.
    stream: S,
    /// Flag which indicates whether the underlying stream has finished, whether with an error or
    /// with a regular EOF. Placeholder for a `Fuse` so that polling after an error or EOF is safe.
    is_finished: bool,
    /// Holds the frame and channel, if available, which has been read by a `DemultiplexerHandle`
    /// corresponding to a different channel.
    next_frame: Option<(u8, Bytes)>,
    /// A bit-field representing the channels which have had `DemultiplexerHandle`s constructed.
    active_channels: [u8; CHANNEL_BYTE_COUNT],
    /// An array of `Waker`s for each channel.
    wakers: [Option<Waker>; MAX_CHANNELS],
}

impl<S: Stream> Demultiplexer<S> {
    /// Creates a new demultiplexer with the given underlying stream.
    pub fn new(stream: S) -> Demultiplexer<S> {
        const WAKERS_INIT: Option<Waker> = None;
        Demultiplexer {
            // We fuse the stream in case its unsafe to call it after yielding `Poll::Ready(None)`
            stream,
            is_finished: false,
            // Initially, we have no next frame
            next_frame: None,
            // Initially, all channels are inactive
            active_channels: [0b00000000; CHANNEL_BYTE_COUNT],
            // Wakers list, one for each channel
            wakers: [WAKERS_INIT; MAX_CHANNELS],
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

    fn wake_pending_channels(&mut self) {
        for maybe_waker in self.wakers.iter_mut() {
            if let Some(waker) = maybe_waker.take() {
                waker.wake();
            }
        }
    }

    fn on_stream_close(&mut self) {
        self.is_finished = true;
        self.wake_pending_channels();
    }

    /// Creates a handle listening for frames on the given channel.
    ///
    /// Items received through a given handle may be blocked if other handles on the same
    /// Demultiplexer are not polled at the same time. Duplicate handles on the same channel
    /// are not allowed.
    ///
    /// Notice: Once a handle was created, it must be constantly polled for the next item
    /// until the end of the stream, after which it should be dropped. If a channel yields
    /// a `Poll::Ready` and it is not polled further, the other channels will stall as they
    /// will never receive a wake. Also, once the end of the stream has been detected on a
    /// channel, it will notify all other pending channels through wakes, but in order for
    /// this to happen the user must either keep calling `handle.next().await` or finally
    /// drop the handle.
    pub fn create_handle<E>(
        demux: Arc<Mutex<Self>>,
        channel: u8,
    ) -> Result<DemultiplexerHandle<S>, DemultiplexerError<E>>
    where
        E: Error,
    {
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
        let mut demux = self.demux.lock().expect("poisoned lock");
        demux.wakers[self.channel as usize] = None;
        demux.wake_pending_channels();
        demux.deactivate_channel(self.channel);
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
        // Unchecked access is safe because the `Vec` was preallocated with necessary elements.
        demux.wakers[self.channel as usize] = None;

        // If next_frame has a suitable frame for this channel, return it in a `Poll::Ready`. If it
        // has an unsuitable frame, return `Poll::Pending`. Otherwise, we attempt to read
        // from the stream.
        if let Some((channel, ref bytes)) = demux.next_frame {
            if channel == self.channel {
                let bytes = bytes.clone();
                demux.next_frame = None;
                return Poll::Ready(Some(Ok(bytes)));
            } else {
                // Wake the channel this frame is for while also deregistering its
                // waker from the list.
                if let Some(waker) = demux.wakers[channel as usize].take() {
                    waker.wake()
                }
                // Before returning `Poll::Pending`, register this channel's waker
                // so that other channels can wake it up when it receives a frame.
                demux.wakers[self.channel as usize] = Some(cx.waker().clone());
                return Poll::Pending;
            }
        }

        if demux.is_finished {
            return Poll::Ready(None);
        }

        // Try to read from the stream, placing the frame into `next_frame` and returning
        // `Poll::Pending` if it's in the wrong channel, otherwise returning it in a
        // `Poll::Ready`.
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
                } else if demux.channel_is_active(channel) {
                    demux.next_frame = Some((channel, bytes));
                    // Wake the channel this frame is for while also deregistering its
                    // waker from the list.
                    if let Some(waker) = demux.wakers[channel as usize].take() {
                        waker.wake();
                    }
                    // Before returning `Poll::Pending`, register this channel's waker
                    // so that other channels can wake it up when it receives a frame.
                    demux.wakers[self.channel as usize] = Some(cx.waker().clone());
                    Poll::Pending
                } else {
                    Poll::Ready(Some(Err(DemultiplexerError::ChannelNotActive(channel))))
                }
            }
            Some(Err(err)) => {
                // Mark the stream as closed when receiving an error from the
                // underlying stream.
                demux.on_stream_close();
                Poll::Ready(Some(Err(DemultiplexerError::Stream(err))))
            }
            None => {
                demux.on_stream_close();
                Poll::Ready(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Error as IoError;

    use crate::testing::TestingStream;

    use super::*;
    use bytes::BytesMut;
    use futures::{FutureExt, StreamExt};

    impl<E: Error> PartialEq for DemultiplexerError<E> {
        fn eq(&self, other: &Self) -> bool {
            match (self, other) {
                (Self::ChannelNotActive(l0), Self::ChannelNotActive(r0)) => l0 == r0,
                (Self::ChannelUnavailable(l0), Self::ChannelUnavailable(r0)) => l0 == r0,
                (Self::MissingFrame(l0), Self::MissingFrame(r0)) => l0 == r0,
                _ => core::mem::discriminant(self) == core::mem::discriminant(other),
            }
        }
    }

    #[test]
    fn channel_activation() {
        let items: Vec<Result<Bytes, DemultiplexerError<IoError>>> = vec![];
        let stream = TestingStream::new(items);
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
        let stream = TestingStream::new(items);
        let demux = Arc::new(Mutex::new(Demultiplexer::new(stream)));

        // We make two handles, one for the 0 channel and another for the 1 channel
        let mut zero_handle = Demultiplexer::create_handle::<IoError>(demux.clone(), 0).unwrap();
        let mut one_handle = Demultiplexer::create_handle::<IoError>(demux, 1).unwrap();

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

    #[test]
    fn single_handle_per_channel() {
        let stream: TestingStream<()> = TestingStream::new(Vec::new());
        let demux = Arc::new(Mutex::new(Demultiplexer::new(stream)));

        // Creating a handle for a channel works.
        let _handle = Demultiplexer::create_handle::<IoError>(demux.clone(), 0).unwrap();
        match Demultiplexer::create_handle::<IoError>(demux.clone(), 0) {
            Err(DemultiplexerError::ChannelUnavailable(0)) => {}
            _ => panic!("Channel 0 was available even though we already have a handle to it"),
        }
        assert!(Demultiplexer::create_handle::<IoError>(demux, 1).is_ok());
    }

    // #[test]
    // fn all_channels_pending_initially() {
    //     let stream: TestStream<()> = TestStream::new(Vec::new());
    //     let demux = Arc::new(Mutex::new(Demultiplexer::new(stream)));

    //     let zero_handle = Demultiplexer::create_handle::<IoError>(demux.clone(), 0).unwrap();

    //     let one_handle = Demultiplexer::create_handle::<IoError>(demux.clone(), 1).unwrap();
    // }

    #[tokio::test]
    async fn concurrent_channels_on_different_tasks() {
        let items: Vec<Result<Bytes, DemultiplexerError<IoError>>> = [
            Bytes::copy_from_slice(&[0, 1, 2, 3, 4]),
            Bytes::copy_from_slice(&[0, 5, 6]),
            Bytes::copy_from_slice(&[1, 101, 102]),
            Bytes::copy_from_slice(&[1, 103, 104]),
            Bytes::copy_from_slice(&[2, 201, 202]),
            Bytes::copy_from_slice(&[0, 7]),
            Bytes::copy_from_slice(&[2, 203, 204]),
            Bytes::copy_from_slice(&[1, 105]),
        ]
        .into_iter()
        .map(Result::Ok)
        .collect();
        let stream = TestingStream::new(items);
        let demux = Arc::new(Mutex::new(Demultiplexer::new(stream)));

        let handle_0 = Demultiplexer::create_handle::<IoError>(demux.clone(), 0).unwrap();
        let handle_1 = Demultiplexer::create_handle::<IoError>(demux.clone(), 1).unwrap();
        let handle_2 = Demultiplexer::create_handle::<IoError>(demux.clone(), 2).unwrap();

        let channel_0_bytes = tokio::spawn(async {
            let mut acc = BytesMut::new();
            handle_0
                .for_each(|bytes| {
                    acc.extend(bytes.unwrap());
                    futures::future::ready(())
                })
                .await;
            acc.freeze()
        });
        let channel_1_bytes = tokio::spawn(async {
            let mut acc = BytesMut::new();
            handle_1
                .for_each(|bytes| {
                    acc.extend(bytes.unwrap());
                    futures::future::ready(())
                })
                .await;
            acc.freeze()
        });
        let channel_2_bytes = tokio::spawn(async {
            let mut acc = BytesMut::new();
            handle_2
                .for_each(|bytes| {
                    acc.extend(bytes.unwrap());
                    futures::future::ready(())
                })
                .await;
            acc.freeze()
        });

        let (result1, result2, result3) =
            tokio::join!(channel_0_bytes, channel_1_bytes, channel_2_bytes,);
        assert_eq!(result1.unwrap(), &[1, 2, 3, 4, 5, 6, 7][..]);
        assert_eq!(result2.unwrap(), &[101, 102, 103, 104, 105][..]);
        assert_eq!(result3.unwrap(), &[201, 202, 203, 204][..]);
    }
}
