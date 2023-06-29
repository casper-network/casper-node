//! Multiframe reading support.
//!
//! The juliet protocol supports multi-frame messages, which are subject to addtional rules and
//! checks. The resulting state machine is encoded in the [`MultiframeReceiver`] type.

use std::{marker::PhantomData, mem, ops::Deref};

use bytes::{Buf, BytesMut};

use crate::{
    header::{ErrorKind, Header},
    reader::Outcome::{self, Fatal, Success},
    try_outcome,
    varint::decode_varint32,
};

/// Bytes offset with a lifetime.
///
/// Helper type that ensures that offsets that are depending on a buffer are not being invalidated through accidental modification.
struct Index<'a> {
    /// The byte offset this `Index` represents.
    index: usize,
    /// Buffer it is tied to.
    buffer: PhantomData<&'a BytesMut>,
}

impl<'a> Deref for Index<'a> {
    type Target = usize;

    fn deref(&self) -> &Self::Target {
        &self.index
    }
}

impl<'a> Index<'a> {
    /// Creates a new `Index` with offset value `index`, borrowing `buffer`.
    fn new(buffer: &'a BytesMut, index: usize) -> Self {
        let _ = buffer;
        Index {
            index,
            buffer: PhantomData,
        }
    }
}

/// The multi-frame message receival state of a single channel, as specified in the RFC.
#[derive(Debug, Default)]
pub(super) enum MultiframeReceiver {
    /// The channel is ready to start receiving a new multi-frame message.
    #[default]
    Ready,
    /// A multi-frame message transfer is currently in progress.
    InProgress {
        /// The header that initiated the multi-frame transfer.
        header: Header,
        /// Payload data received so far.
        payload: BytesMut,
        /// The total size of the payload to be received.
        total_payload_size: u32,
    },
}

impl MultiframeReceiver {
    /// Attempt to process a single multi-frame message frame.
    ///
    /// The caller MUST only call this method if it has determined that the frame in `buffer` is one
    /// that includes a payload. If this is the case, the entire receive `buffer` should be passed
    /// to this function.
    ///
    /// If a message payload matching the given header has been succesfully completed, both header
    /// and payload are consumed from the `buffer`, the payload being returned. If a starting or
    /// intermediate segment was processed without completing the message, both are still consume,
    /// but `None` is returned instead. This method will never consume more than one frame.
    ///
    /// On any error, [`Outcome::Err`] with a suitable header to return to the sender is returned.
    ///
    /// `max_payload_size` is the maximum size of a payload across multiple frames. If it is
    /// exceeded, the `payload_exceeded_error_kind` function is used to construct an error `Header`
    /// to return.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `max_frame_size` is too small to hold a maximum sized varint and
    /// a header.
    pub(super) fn accept(
        &mut self,
        header: Header,
        buffer: &mut BytesMut,
        max_frame_size: u32,
        max_payload_size: u32,
        payload_exceeded_error_kind: ErrorKind,
    ) -> Outcome<Option<BytesMut>, Header> {
        debug_assert!(
            max_frame_size >= 10,
            "maximum frame size must be enough to hold header and varint"
        );

        match self {
            MultiframeReceiver::Ready => {
                // We have a new segment, which has a variable size.
                let segment_buf = &buffer[Header::SIZE..];

                let payload_size = try_outcome!(decode_varint32(segment_buf)
                    .map_err(|_overflow| header.with_err(ErrorKind::BadVarInt)));

                {
                    {
                        if payload_size.value > max_payload_size {
                            return Fatal(header.with_err(payload_exceeded_error_kind));
                        }

                        // We have a valid varint32.
                        let preamble_size = Header::SIZE as u32 + payload_size.offset.get() as u32;
                        let max_data_in_frame = (max_frame_size - preamble_size) as u32;

                        // Determine how many additional bytes are needed for frame completion.
                        let frame_end = Index::new(
                            &buffer,
                            preamble_size as usize
                                + (max_data_in_frame as usize).min(payload_size.value as usize),
                        );
                        if buffer.remaining() < *frame_end {
                            return Outcome::incomplete(buffer.remaining() - *frame_end);
                        }

                        // At this point we are sure to complete a frame, so drop the preamble.
                        buffer.advance(preamble_size as usize);

                        // Is the payload complete in one frame?
                        if payload_size.value <= max_data_in_frame {
                            let payload = buffer.split_to(payload_size.value as usize);

                            // No need to alter the state, we stay `Ready`.
                            Success(Some(payload))
                        } else {
                            // Length exceeds the frame boundary, split to maximum and store that.
                            let partial_payload = buffer.split_to(max_frame_size as usize);

                            // We are now in progress of reading a payload.
                            *self = MultiframeReceiver::InProgress {
                                header,
                                payload: partial_payload,
                                total_payload_size: payload_size.value,
                            };

                            // We have successfully consumed a frame, but are not finished yet.
                            Success(None)
                        }
                    }
                }
            }
            MultiframeReceiver::InProgress {
                header: active_header,
                payload,
                total_payload_size,
            } => {
                if header != *active_header {
                    // The newly supplied header does not match the one active.
                    return Fatal(header.with_err(ErrorKind::InProgress));
                }

                // Determine whether we expect an intermediate or end segment.
                let bytes_remaining = *total_payload_size as usize - payload.remaining();
                let max_data_in_frame = max_frame_size as usize - Header::SIZE;

                if bytes_remaining > max_data_in_frame {
                    // Intermediate segment.
                    if buffer.remaining() < max_frame_size as usize {
                        return Outcome::incomplete(max_frame_size as usize - buffer.remaining());
                    }

                    // Discard header.
                    buffer.advance(Header::SIZE);

                    // Copy data over to internal buffer.
                    payload.extend_from_slice(&buffer[0..max_data_in_frame]);
                    buffer.advance(max_data_in_frame);

                    // We're done with this frame (but not the payload).
                    Success(None)
                } else {
                    // End segment
                    let frame_end = Index::new(&buffer, bytes_remaining + Header::SIZE);

                    // If we don't have the entire frame read yet, return.
                    if *frame_end > buffer.remaining() {
                        return Outcome::incomplete(*frame_end - buffer.remaining());
                    }

                    // Discard header.
                    buffer.advance(Header::SIZE);

                    // Copy data over to internal buffer.
                    payload.extend_from_slice(&buffer[0..bytes_remaining]);
                    buffer.advance(bytes_remaining);

                    let finished_payload = mem::take(payload);
                    *self = MultiframeReceiver::Ready;

                    Success(Some(finished_payload))
                }
            }
        }
    }

    /// Determines whether given `new_header` would be a new transfer if accepted.
    ///
    /// If `false`, `new_header` would indicate a continuation of an already in-progress transfer.
    pub(super) fn is_new_transfer(&self, new_header: Header) -> bool {
        match self {
            MultiframeReceiver::Ready => true,
            MultiframeReceiver::InProgress { header, .. } => *header != new_header,
        }
    }
}
