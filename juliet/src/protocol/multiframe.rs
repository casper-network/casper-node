//! Multiframe reading support.
//!
//! The juliet protocol supports multi-frame messages, which are subject to addtional rules and
//! checks. The resulting state machine is encoded in the [`MultiframeReceiver`] type.

use std::mem;

use bytes::{Buf, BytesMut};

use crate::{
    header::{ErrorKind, Header},
    protocol::{
        err_msg,
        Outcome::{self, Success},
    },
    try_outcome,
    util::Index,
    varint::decode_varint32,
};

use super::outgoing_message::OutgoingMessage;

/// The multi-frame message receival state of a single channel, as specified in the RFC.
///
/// The receiver is not channel-aware, that is it will treat a new multi-frame message on a channel
/// that is different from the one where a multi-frame transfer is already in progress as an error
/// in the same way it would if they were on the same channel. The caller thus must ensure to create
/// an instance of `MultiframeReceiver` for every active channel.
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
    /// intermediate segment was processed without completing the message, both are still consumed,
    /// but `None` is returned instead. This method will never consume more than one frame.
    ///
    /// On any error, [`Outcome::Err`] with a suitable message to return to the sender is returned.
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
    ) -> Outcome<Option<BytesMut>, OutgoingMessage> {
        debug_assert!(
            max_frame_size >= 10,
            "maximum frame size must be enough to hold header and varint"
        );

        // TODO: Use tracing to log frames here.

        match self {
            MultiframeReceiver::Ready => {
                // We have a new segment, which has a variable size.
                let segment_buf = &buffer[Header::SIZE..];

                let payload_size =
                    try_outcome!(decode_varint32(segment_buf).map_err(|_overflow| {
                        OutgoingMessage::new(header.with_err(ErrorKind::BadVarInt), None)
                    }));

                if payload_size.value > max_payload_size {
                    return err_msg(header, payload_exceeded_error_kind);
                }

                // We have a valid varint32.
                let preamble_size = Header::SIZE as u32 + payload_size.offset.get() as u32;
                let max_data_in_frame = max_frame_size - preamble_size;

                // Determine how many additional bytes are needed for frame completion.
                let frame_end = Index::new(
                    buffer,
                    preamble_size as usize
                        + (max_data_in_frame as usize).min(payload_size.value as usize),
                );
                if buffer.remaining() < *frame_end {
                    return Outcome::incomplete(*frame_end - buffer.remaining());
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
                    let partial_payload = buffer.split_to(max_data_in_frame as usize);

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
            MultiframeReceiver::InProgress {
                header: active_header,
                payload,
                total_payload_size,
            } => {
                if header != *active_header {
                    // The newly supplied header does not match the one active.
                    return err_msg(header, ErrorKind::InProgress);
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
                    let frame_end = Index::new(buffer, bytes_remaining + Header::SIZE);

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

#[cfg(test)]
mod tests {
    use bytes::{BufMut, Bytes, BytesMut};
    use proptest::{arbitrary::any, collection, proptest};
    use proptest_derive::Arbitrary;

    use crate::{
        header::{ErrorKind, Header, Kind},
        protocol::{FrameIter, OutgoingMessage},
        ChannelId, Id, Outcome,
    };

    use super::MultiframeReceiver;

    /// Frame size used for multiframe tests.
    const MAX_FRAME_SIZE: u32 = 16;

    /// Maximum size of a payload of a single frame message.
    ///
    /// One byte is required to encode the length, which is <= 16.
    const MAX_SINGLE_FRAME_PAYLOAD_SIZE: u32 = MAX_FRAME_SIZE - Header::SIZE as u32 - 1;

    /// Maximum payload size used in testing.
    const MAX_PAYLOAD_SIZE: u32 = 4096;

    #[test]
    fn single_message_frame_by_frame() {
        // We single-feed a message frame-by-frame into the multi-frame receiver:
        let mut receiver = MultiframeReceiver::default();

        let payload = gen_payload(64);
        let header = Header::new(Kind::RequestPl, ChannelId(1), Id(1));

        let msg = OutgoingMessage::new(header, Some(Bytes::from(payload.clone())));

        let mut buffer = BytesMut::new();
        let mut frames_left = msg.num_frames(MAX_FRAME_SIZE);

        for frame in msg.frame_iter(MAX_FRAME_SIZE) {
            assert!(frames_left > 0);
            frames_left -= 1;

            buffer.put(frame);

            match receiver.accept(
                header,
                &mut buffer,
                MAX_FRAME_SIZE,
                MAX_PAYLOAD_SIZE,
                ErrorKind::RequestLimitExceeded,
            ) {
                Outcome::Incomplete(n) => {
                    assert_eq!(n.get(), 4, "expected multi-frame to ask for header next");
                }
                Outcome::Fatal(_) => {
                    panic!("did not expect fatal error on multi-frame parse")
                }
                Outcome::Success(Some(output)) => {
                    assert_eq!(frames_left, 0, "should have consumed all frames");
                    assert_eq!(output, payload);
                }
                Outcome::Success(None) => {
                    // all good, we will read another frame
                }
            }
            assert!(
                buffer.is_empty(),
                "multi frame receiver should consume entire frame"
            );
        }
    }

    /// A testing model action .
    #[derive(Arbitrary, derive_more::Debug)]
    enum Action {
        /// Sends a single frame not subject to multi-frame (due to its payload fitting the size).
        #[proptest(weight = 30)]
        SendSingleFrame {
            /// Header for the single frame.
            ///
            /// Subject to checking for conflicts with ongoing multi-frame messages.
            header: Header,
            /// The payload to include.
            #[proptest(
                strategy = "collection::vec(any::<u8>(), 0..=MAX_SINGLE_FRAME_PAYLOAD_SIZE as usize)"
            )]
            #[debug("{} bytes", payload.len())]
            payload: Vec<u8>,
        },
        /// Creates a new multi-frame message, does nothing if there is already one in progress.
        #[proptest(weight = 5)]
        BeginMultiFrameMessage {
            /// Header for the new multi-frame message.
            header: Header,
            /// Payload to include.
            #[proptest(
                strategy = "collection::vec(any::<u8>(), (MAX_SINGLE_FRAME_PAYLOAD_SIZE as usize+1)..=MAX_PAYLOAD_SIZE as usize)"
            )]
            #[debug("{} bytes", payload.len())]
            payload: Vec<u8>,
        },
        /// Continue sending the current multi-frame message; does nothing if no multi-frame send
        /// is in progress.
        #[proptest(weight = 63)]
        Continue,
        /// Creates a multi-frame message that conflicts with one already in progress. If there is
        /// no transfer in progress, does nothing.
        #[proptest(weight = 1)]
        SendConflictingMultiFrameMessage {
            /// Header for the conflicting multi-frame message.
            ///
            /// Will be adjusted if NOT conflicting.
            header: Header,
            /// Size of the payload to include.
            #[proptest(
                strategy = "collection::vec(any::<u8>(), (MAX_SINGLE_FRAME_PAYLOAD_SIZE as usize+1)..=MAX_PAYLOAD_SIZE as usize)"
            )]
            #[debug("{} bytes", payload.len())]
            payload: Vec<u8>,
        },
        /// Sends another frame with data.
        ///
        /// Will be ignored if hitting the last frame of the payload.
        #[proptest(weight = 1)]
        ContinueWithoutTooSmallFrame,
        /// Exceeds the size limit.
        #[proptest(weight = 1)]
        ExceedPayloadSizeLimit {
            /// The header for the new message.
            header: Header,
            /// How much to reduce the maximum payload size by.
            #[proptest(strategy = "collection::vec(any::<u8>(),
                    (MAX_SINGLE_FRAME_PAYLOAD_SIZE as usize + 1)
                    ..=(2+2*MAX_SINGLE_FRAME_PAYLOAD_SIZE as usize))")]
            #[debug("{} bytes", payload.len())]
            payload: Vec<u8>,
        },
    }

    proptest! {
    #[test]
    fn model_sequence_test_multi_frame_receiver(
        actions in collection::vec(any::<Action>(), 0..1000)
    ) {
        let (input, expected) = generate_model_sequence(actions);
        check_model_sequence(input, expected)
    }
    }

    /// Creates a new header guaranteed to be different from the given header.
    fn twiddle_header(header: Header) -> Header {
        let new_id = Id::new(header.id().get().wrapping_add(1));
        if header.is_error() {
            Header::new_error(header.error_kind(), header.channel(), new_id)
        } else {
            Header::new(header.kind(), header.channel(), new_id)
        }
    }

    /// Generates a model sequence and encodes it as input.
    ///
    /// Returns a [`BytesMut`] buffer filled with a syntactically valid sequence of bytes that
    /// decode to multiple frames, along with vector of expected outcomes of the
    /// [`MultiframeReceiver::accept`] method.
    fn generate_model_sequence(
        actions: Vec<Action>,
    ) -> (BytesMut, Vec<Outcome<Option<BytesMut>, OutgoingMessage>>) {
        let mut expected = Vec::new();

        let mut active_transfer: Option<FrameIter> = None;
        let mut active_payload = Vec::new();
        let mut input = BytesMut::new();

        for action in actions {
            match action {
                Action::SendSingleFrame {
                    mut header,
                    payload,
                } => {
                    // Ensure the new message does not clash with an ongoing transfer.
                    if let Some(ref active_transfer) = active_transfer {
                        if active_transfer.header() == header {
                            header = twiddle_header(header);
                        }
                    }

                    // Sending a standalone frame should yield a message instantly.
                    let pl = BytesMut::from(payload.as_slice());
                    expected.push(Outcome::Success(Some(pl)));
                    input.put(
                        OutgoingMessage::new(header, Some(payload.into()))
                            .iter_bytes(MAX_FRAME_SIZE),
                    );
                }
                Action::BeginMultiFrameMessage { header, payload } => {
                    if active_transfer.is_some() {
                        // Do not create conflicts, just ignore.
                        continue;
                    }

                    // Construct iterator over multi-frame message.
                    let frames =
                        OutgoingMessage::new(header, Some(payload.clone().into())).frames();
                    active_payload = payload;

                    // The first read will be a `None` read.
                    expected.push(Outcome::Success(None));
                    let (frame, more) = frames.next_owned(MAX_FRAME_SIZE);
                    input.put(frame);

                    active_transfer = Some(
                        more.expect("test generated multi-frame message that only has one frame"),
                    );
                }
                Action::Continue => match active_transfer.take() {
                    Some(frames) => {
                        let (frame, more) = frames.next_owned(MAX_FRAME_SIZE);

                        if more.is_some() {
                            // More frames to come.
                            expected.push(Outcome::Success(None));
                        } else {
                            let pl = BytesMut::from(active_payload.as_slice());
                            expected.push(Outcome::Success(Some(pl)));
                        }

                        input.put(frame);
                        active_transfer = more;
                    }
                    None => {
                        // Nothing to do - there is no transfer to continue.
                    }
                },
                Action::SendConflictingMultiFrameMessage {
                    mut header,
                    payload,
                } => {
                    if let Some(ref active_transfer) = active_transfer {
                        // Ensure we don't accidentally hit the same header.
                        if active_transfer.header() == header {
                            header = twiddle_header(header);
                        }

                        // We were asked to produce an error, since the protocol was violated.
                        let msg = OutgoingMessage::new(header, Some(payload.into()));
                        let (frame, _) = msg.frames().next_owned(MAX_FRAME_SIZE);
                        input.put(frame);
                        expected.push(Outcome::Fatal(OutgoingMessage::new(
                            header.with_err(ErrorKind::InProgress),
                            None,
                        )));
                        break; // Stop after error.
                    } else {
                        // Nothing to do - we cannot conflict with a transfer if there is none.
                    }
                }
                Action::ContinueWithoutTooSmallFrame => {
                    if let Some(ref active_transfer) = active_transfer {
                        let header = active_transfer.header();

                        // The only guarantee we have is that there is at least one more byte of
                        // payload, so we send a zero-sized payload.
                        let msg = OutgoingMessage::new(header, Some(Bytes::new()));
                        let (frame, _) = msg.frames().next_owned(MAX_FRAME_SIZE);
                        input.put(frame);
                        expected.push(Outcome::Fatal(OutgoingMessage::new(
                            header.with_err(ErrorKind::SegmentViolation),
                            None,
                        )));
                        break; // Stop after error.
                    } else {
                        // Nothing to do, we cannot send a too-small frame if there is no transfer.
                    }
                }
                Action::ExceedPayloadSizeLimit { header, payload } => {
                    if active_transfer.is_some() {
                        // Only do this if there is no active transfer.
                        continue;
                    }

                    let msg = OutgoingMessage::new(header, Some(payload.into()));
                    let (frame, _) = msg.frames().next_owned(MAX_FRAME_SIZE);
                    input.put(frame);
                    expected.push(Outcome::Fatal(OutgoingMessage::new(
                        header.with_err(ErrorKind::RequestTooLarge),
                        None,
                    )));
                    break;
                }
            }
        }

        (input, expected)
    }

    /// Extracts a header from a slice.
    ///
    /// # Panics
    ///
    /// Panics if there is no syntactically well-formed header in the first four bytes of `data`.
    #[track_caller]
    fn expect_header_from_slice(data: &[u8]) -> Header {
        let raw_header: [u8; Header::SIZE] =
            <[u8; Header::SIZE] as TryFrom<&[u8]>>::try_from(&data[..Header::SIZE])
                .expect("did not expect header to be missing")
                .clone();
        Header::parse(raw_header).expect("did not expect header parsing to fail")
    }

    /// Process a given input and compare it against predetermined expected outcomes.
    fn check_model_sequence(
        mut input: BytesMut,
        expected: Vec<Outcome<Option<BytesMut>, OutgoingMessage>>,
    ) {
        let mut receiver = MultiframeReceiver::default();

        let mut actual = Vec::new();
        while !input.is_empty() {
            // We need to perform the work usually done by the IO system and protocol layer before
            // we can pass it on to the multi-frame handler.
            let header = expect_header_from_slice(&input);

            let outcome = receiver.accept(
                header,
                &mut input,
                MAX_FRAME_SIZE,
                MAX_PAYLOAD_SIZE,
                ErrorKind::RequestTooLarge,
            );
            actual.push(outcome);

            // On error, we exit.
            if matches!(actual.last().unwrap(), Outcome::Fatal(_)) {
                break;
            }
        }

        assert_eq!(actual, expected);
        assert!(input.is_empty(), "error should be last message");
    }

    /// Generates a payload.
    fn gen_payload(size: usize) -> Vec<u8> {
        let mut payload = Vec::with_capacity(size);
        for i in 0..size {
            payload.push((i % 256) as u8);
        }
        payload
    }

    #[test]
    fn mutltiframe_allows_interspersed_frames() {
        let sf_payload = gen_payload(10);

        let actions = vec![
            Action::BeginMultiFrameMessage {
                header: Header::new(Kind::Request, ChannelId(0), Id(0)),
                payload: gen_payload(1361),
            },
            Action::SendSingleFrame {
                header: Header::new_error(ErrorKind::Other, ChannelId(1), Id(42188)),
                payload: sf_payload.clone(),
            },
        ];

        // Failed sequence was generated by a proptest, check that it matches.
        assert_eq!(format!("{:?}", actions), "[BeginMultiFrameMessage { header: [Request chan: 0 id: 0], payload: 1361 bytes }, SendSingleFrame { header: [err:Other chan: 1 id: 42188], payload: 10 bytes }]");

        let (input, expected) = generate_model_sequence(actions);

        // We expect the single frame message to come through.
        assert_eq!(
            expected,
            vec![
                Outcome::Success(None),
                Outcome::Success(Some(sf_payload.as_slice().into()))
            ]
        );

        check_model_sequence(input, expected);
    }
}
