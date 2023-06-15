use std::{marker::PhantomData, mem, ops::Deref};

use bytes::{Buf, BytesMut};

use crate::{
    header::{ErrorKind, Header},
    reader::Outcome::{self, Incomplete, Success},
    varint::{decode_varint32, Varint32Result},
};

/// Bytes offset with a lifetime.
///
/// Ensures that offsets that are depending on a buffer not being modified are not invalidated.
struct Index<'a> {
    /// The value of the `Index`.
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
    /// Creates a new `Index` with value `index`, borrowing `buffer`.
    fn new(buffer: &'a BytesMut, index: usize) -> Self {
        let _ = buffer;
        Index {
            index,
            buffer: PhantomData,
        }
    }
}

/// The multi-frame message receival state of a single channel, as specified in the RFC.
#[derive(Debug)]
pub(super) enum MultiframeSendState {
    /// The channel is ready to start receiving a new multi-frame message.
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

impl MultiframeSendState {
    /// Attempt to process a single multi-frame message frame.
    ///
    /// The caller must only calls this method if it has determined that the frame in `buffer` is
    /// one that requires a payload.
    ///
    /// If a message payload matching the given header has been succesfully completed, returns it.
    /// If a starting or intermediate segment was processed without completing the message, returns
    /// `None` instead. This method will never consume more than one frame.
    ///
    /// Assumes that `header` is the first [`Header::SIZE`] bytes of `buffer`. Will advance `buffer`
    /// past header and payload only on success.
    pub(super) fn accept(
        &mut self,
        header: Header,
        buffer: &mut BytesMut,
        max_frame_size: u32,
    ) -> Outcome<Option<BytesMut>> {
        debug_assert!(
            max_frame_size >= 10,
            "maximum frame size must be enough to hold header and varint"
        );

        match self {
            MultiframeSendState::Ready => {
                // We have a new segment, which has a variable size.
                let segment_buf = &buffer[Header::SIZE..];

                match decode_varint32(segment_buf) {
                    Varint32Result::Incomplete => return Incomplete(1),
                    Varint32Result::Overflow => return header.return_err(ErrorKind::BadVarInt),
                    Varint32Result::Valid {
                        offset,
                        value: total_payload_size,
                    } => {
                        // We have a valid varint32.
                        let preamble_size = Header::SIZE as u32 + offset.get() as u32;
                        let max_data_in_frame = (max_frame_size - preamble_size) as u32;

                        // Determine how many additional bytes are needed for frame completion.
                        let frame_end = Index::new(
                            &buffer,
                            preamble_size as usize
                                + (max_data_in_frame as usize).min(total_payload_size as usize),
                        );
                        if buffer.remaining() < *frame_end {
                            return Incomplete(buffer.remaining() - *frame_end);
                        }

                        // At this point we are sure to complete a frame, so drop the preamble.
                        buffer.advance(preamble_size as usize);

                        // Is the payload complete in one frame?
                        if total_payload_size <= max_data_in_frame {
                            let payload = buffer.split_to(total_payload_size as usize);

                            // No need to alter the state, we stay `Ready`.
                            Success(Some(payload))
                        } else {
                            // Length exceeds the frame boundary, split to maximum and store that.
                            let partial_payload = buffer.split_to(max_frame_size as usize);

                            // We are now in progress of reading a payload.
                            *self = MultiframeSendState::InProgress {
                                header,
                                payload: partial_payload,
                                total_payload_size,
                            };

                            // We have successfully consumed a frame, but are not finished yet.
                            Success(None)
                        }
                    }
                }
            }
            MultiframeSendState::InProgress {
                header: active_header,
                payload,
                total_payload_size,
            } => {
                if header != *active_header {
                    // The newly supplied header does not match the one active.
                    return header.return_err(ErrorKind::InProgress);
                }

                // Determine whether we expect an intermediate or end segment.
                let bytes_remaining = *total_payload_size as usize - payload.remaining();
                let max_data_in_frame = max_frame_size as usize - Header::SIZE;

                if bytes_remaining > max_data_in_frame {
                    // Intermediate segment.
                    if buffer.remaining() < max_frame_size as usize {
                        return Incomplete(max_frame_size as usize - buffer.remaining());
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
                        return Incomplete(*frame_end - buffer.remaining());
                    }

                    // Discard header.
                    buffer.advance(Header::SIZE);

                    // Copy data over to internal buffer.
                    payload.extend_from_slice(&buffer[0..bytes_remaining]);
                    buffer.advance(bytes_remaining);

                    let finished_payload = mem::take(payload);
                    *self = MultiframeSendState::Ready;

                    Success(Some(finished_payload))
                }
            }
        }
    }

    #[inline]
    pub(super) fn current_header(&self) -> Option<Header> {
        match self {
            MultiframeSendState::Ready => None,
            MultiframeSendState::InProgress { header, .. } => Some(*header),
        }
    }

    pub(super) fn is_new_transfer(&self, new_header: Header) -> bool {
        match self {
            MultiframeSendState::Ready => true,
            MultiframeSendState::InProgress { header, .. } => *header != new_header,
        }
    }
}
