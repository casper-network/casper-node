//! Outgoing message data.
//!
//! The [`protocol`](crate::protocol) module exposes a pure, non-IO state machine for handling the
//! juliet networking protocol, this module contains the necessary output types like
//! [`OutgoingMessage`].

use std::{
    fmt::{self, Debug, Display, Formatter, Write},
    io::Cursor,
};

use bytemuck::{Pod, Zeroable};
use bytes::{buf::Chain, Buf, BufMut, Bytes};

use crate::{header::Header, varint::Varint32};

use super::payload_is_multi_frame;

/// A message to be sent to the peer.
///
/// [`OutgoingMessage`]s are generated when the protocol requires data to be sent to the peer.
/// Unless the connection is terminated, they should not be dropped, but can be sent in any order.
///
/// While *frames* can be sent in any order, a message may span one or more frames, which can be
/// interspersed with other messages at will. In general, the [`OutgoingMessage::frames()`] iterator
/// should be used, even for single-frame messages.
#[must_use]
#[derive(Clone, Debug)]
pub struct OutgoingMessage {
    /// The common header for all outgoing messages.
    header: Header,
    /// The payload, potentially split across multiple messages.
    payload: Option<Bytes>,
}

impl OutgoingMessage {
    /// Constructs a new outgoing messages.
    // Note: Do not make this function available to users of the library, to avoid them constructing
    //       messages by accident that may violate the protocol.
    #[inline(always)]
    pub(super) fn new(header: Header, payload: Option<Bytes>) -> Self {
        Self { header, payload }
    }

    /// Returns whether or not a message will span multiple frames.
    #[inline(always)]
    pub fn is_multi_frame(&self, max_frame_size: u32) -> bool {
        if let Some(ref payload) = self.payload {
            payload_is_multi_frame(max_frame_size, payload.len())
        } else {
            false
        }
    }

    /// Creates an iterator over all frames in the message.
    #[inline(always)]
    pub fn frames(self) -> FrameIter {
        FrameIter {
            msg: self,
            bytes_processed: 0,
        }
    }

    /// Returns the outgoing message's header.
    #[inline(always)]
    pub fn header(&self) -> Header {
        self.header
    }
}

/// Combination of header and potential frame payload length.
///
/// A message with a payload always start with an initial frame that has a header and a varint
/// encoded payload length. This type combines the two, and allows for the payload length to
/// effectively be omitted (through [`Varint32::SENTINEL`]). It has a compact, constant size memory
/// representation regardless of whether a variably sized integer is present or not.
///
/// This type implements [`AsRef<u8>`], which will return the correctly encoded bytes suitable for
/// sending header and potential varint encoded length.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct Preamble {
    /// The header, which is always sent.
    header: Header,
    /// The payload length. If [`Varint32::SENTINEL`], it will always be omitted from output.
    payload_length: Varint32,
}

impl Display for Preamble {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.header, f)?;
        if !self.payload_length.is_sentinel() {
            write!(f, " [l={}]", self.payload_length.decode())?;
        }
        Ok(())
    }
}

impl Preamble {
    /// Creates a new preamble.
    ///
    /// Passing [`Varint32::SENTINEL`] as the length will cause it to be omitted.
    #[inline(always)]
    fn new(header: Header, payload_length: Varint32) -> Self {
        Self {
            header,
            payload_length,
        }
    }

    /// Returns the length of the preamble when encoded as as a bytestring.
    #[inline(always)]
    fn len(self) -> usize {
        Header::SIZE + self.payload_length.len()
    }

    #[inline(always)]
    fn header(self) -> Header {
        self.header
    }
}

impl AsRef<[u8]> for Preamble {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        let bytes = bytemuck::bytes_of(self);
        &bytes[0..(self.len())]
    }
}

/// Iterator over frames of a message.
// Note: This type can be written just borrowing `msg`, by making it owned, we prevent accidental
//       duplicate message sending. Furthermore we allow methods like `into_iter` to be added.
#[derive(Debug)]
#[must_use]
pub struct FrameIter {
    /// The outgoing message in its entirety.
    msg: OutgoingMessage,
    /// Number of bytes output using `OutgoingFrame`s so far.
    bytes_processed: usize,
}

impl FrameIter {
    /// Returns the next frame to send.
    ///
    /// Will return the next frame, and `Some(self)` is there are additional frames to send to
    /// complete the message, `None` otherwise.
    ///
    /// # Note
    ///
    /// While different [`OutgoingMessage`]s can have their send order mixed or interspersed, a
    /// caller MUST NOT send [`OutgoingFrame`]s of a single messagw in any order but the one
    /// produced by this method. In other words, reorder messages, but not frames within a message.
    pub fn next_owned(mut self, max_frame_size: u32) -> (OutgoingFrame, Option<Self>) {
        if let Some(ref payload) = self.msg.payload {
            let mut payload_remaining = payload.len() - self.bytes_processed;

            let length_prefix = if self.bytes_processed == 0 {
                Varint32::encode(payload_remaining as u32)
            } else {
                Varint32::SENTINEL
            };

            let preamble = if self.bytes_processed == 0 {
                Preamble::new(self.msg.header, length_prefix)
            } else {
                Preamble::new(self.msg.header, Varint32::SENTINEL)
            };

            let frame_capacity = max_frame_size as usize - preamble.len();
            let frame_payload_len = frame_capacity.min(payload_remaining);

            let range = self.bytes_processed..(self.bytes_processed + frame_payload_len);
            let frame_payload = payload.slice(range);
            self.bytes_processed += frame_payload_len;

            // Update payload remaining, now that an additional frame has been produced.
            payload_remaining = payload.len() - self.bytes_processed;

            let frame = OutgoingFrame::new_with_payload(preamble, frame_payload);
            if payload_remaining > 0 {
                (frame, Some(self))
            } else {
                (frame, None)
            }
        } else {
            (
                OutgoingFrame::new(Preamble::new(self.msg.header, Varint32::SENTINEL)),
                None,
            )
        }
    }

    /// Writes out all frames as they should be sent out onto the wire into the given buffer.
    ///
    /// This does not leave any way to intersperse other frames and is only recommend in context
    /// like testing.
    #[cfg(test)]
    #[inline]
    pub fn put_into<T: BufMut>(self, buffer: &mut T, max_frame_size: u32) {
        let mut current = self;
        loop {
            let (frame, mut more) = current.next_owned(max_frame_size);

            buffer.put(frame);

            current = if let Some(more) = more.take() {
                more
            } else {
                return;
            }
        }
    }
}

/// A single frame to be sent.
///
/// Implements [`bytes::Buf`], which will yield the bytes to send it across the wire to a peer.
#[derive(Debug)]
#[repr(transparent)]
#[must_use]
pub struct OutgoingFrame(Chain<Cursor<Preamble>, Bytes>);

impl Display for OutgoingFrame {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "<{}", self.0.first_ref().get_ref(),)?;

        let payload = self.0.last_ref();

        if !payload.as_ref().is_empty() {
            f.write_char(' ')?;
            Display::fmt(&crate::util::PayloadFormat(self.0.last_ref()), f)?;
        }

        f.write_str(">")
    }
}

impl OutgoingFrame {
    /// Creates a new [`OutgoingFrame`] with no payload.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if the [`Preamble`] contains a payload length.
    #[inline(always)]
    fn new(preamble: Preamble) -> Self {
        debug_assert!(
            preamble.payload_length.is_sentinel(),
            "frame without payload should not have a payload length"
        );
        Self::new_with_payload(preamble, Bytes::new())
    }

    /// Creates a new [`OutgoingFrame`] with a payload.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if [`Preamble`] does not have a correct payload length, or if the
    /// payload exceeds `u32::MAX` in size.
    #[inline(always)]
    fn new_with_payload(preamble: Preamble, payload: Bytes) -> Self {
        debug_assert!(
            payload.len() <= u32::MAX as usize,
            "payload exceeds maximum allowed payload"
        );

        OutgoingFrame(Cursor::new(preamble).chain(payload))
    }

    /// Returns the outgoing frame's header.
    #[inline]
    pub fn header(&self) -> Header {
        self.0.first_ref().get_ref().header()
    }
}

impl Buf for OutgoingFrame {
    #[inline(always)]
    fn remaining(&self) -> usize {
        self.0.remaining()
    }

    #[inline(always)]
    fn chunk(&self) -> &[u8] {
        self.0.chunk()
    }

    #[inline(always)]
    fn advance(&mut self, cnt: usize) {
        self.0.advance(cnt)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use bytes::{Buf, Bytes, BytesMut};

    use crate::{
        header::{Header, Kind},
        varint::Varint32,
        ChannelId, Id,
    };

    use super::{FrameIter, OutgoingMessage, Preamble};

    /// Maximum frame size used across tests.
    const MAX_FRAME_SIZE: u32 = 16;

    /// A reusable sample payload.
    const PAYLOAD: &[u8] = &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
        48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70,
        71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93,
        94, 95, 96, 97, 98, 99,
    ];

    /// Collects all frames from a single frame iter.
    fn collect_frames(mut iter: FrameIter) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        loop {
            let (mut frame, more) = iter.next_owned(MAX_FRAME_SIZE);
            let expanded = frame.copy_to_bytes(frame.remaining());
            frames.push(expanded.into());
            if let Some(more) = more {
                iter = more;
            } else {
                break frames;
            }
        }
    }

    /// Constructs a message with the given length, turns it into frames and compares if the
    /// resulting frames are equal to the expected frame sequence.
    #[track_caller]
    fn check_payload(length: Option<usize>, expected: &[&[u8]]) {
        assert!(
            !expected.is_empty(),
            "impossible to have message with no frames"
        );

        let payload = length.map(|l| Bytes::from(&PAYLOAD[..l]));

        let header = Header::new(Kind::RequestPl, ChannelId(0xAB), Id(0xEFCD));
        let msg = OutgoingMessage::new(header, payload);

        assert_eq!(msg.header(), header);
        assert_eq!(expected.len() > 1, msg.is_multi_frame(MAX_FRAME_SIZE));

        // A zero-byte payload is still expected to produce a single byte for the 0-length.
        let frames = collect_frames(msg.clone().frames());

        // We could compare without creating a new vec, but this gives nicer error messages.
        let comparable: Vec<_> = frames.iter().map(|v| v.as_slice()).collect();
        assert_eq!(&comparable, expected);

        // Ensure that the written out version is the same as expected.
        let mut written_out = BytesMut::new();
        msg.frames().put_into(&mut written_out, MAX_FRAME_SIZE);
        let expected_bytestring: Vec<u8> = expected
            .into_iter()
            .map(Deref::deref)
            .flatten()
            .copied()
            .collect();
        assert_eq!(written_out, expected_bytestring);
    }

    #[test]
    fn message_is_fragmentized_correctly() {
        check_payload(None, &[&[0x02, 0xAB, 0xCD, 0xEF]]);
        check_payload(Some(0), &[&[0x02, 0xAB, 0xCD, 0xEF, 0]]);
        check_payload(Some(1), &[&[0x02, 0xAB, 0xCD, 0xEF, 1, 0]]);
        check_payload(Some(5), &[&[0x02, 0xAB, 0xCD, 0xEF, 5, 0, 1, 2, 3, 4]]);
        check_payload(
            Some(11),
            &[&[0x02, 0xAB, 0xCD, 0xEF, 11, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]],
        );
        check_payload(
            Some(12),
            &[
                &[0x02, 0xAB, 0xCD, 0xEF, 12, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                &[0x02, 0xAB, 0xCD, 0xEF, 11],
            ],
        );
        check_payload(
            Some(13),
            &[
                &[0x02, 0xAB, 0xCD, 0xEF, 13, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                &[0x02, 0xAB, 0xCD, 0xEF, 11, 12],
            ],
        );
        check_payload(
            Some(23),
            &[
                &[0x02, 0xAB, 0xCD, 0xEF, 23, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                &[
                    0x02, 0xAB, 0xCD, 0xEF, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
                ],
            ],
        );
        check_payload(
            Some(24),
            &[
                &[0x02, 0xAB, 0xCD, 0xEF, 24, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                &[
                    0x02, 0xAB, 0xCD, 0xEF, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
                ],
                &[0x02, 0xAB, 0xCD, 0xEF, 23],
            ],
        );
        check_payload(
            Some(35),
            &[
                &[0x02, 0xAB, 0xCD, 0xEF, 35, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                &[
                    0x02, 0xAB, 0xCD, 0xEF, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
                ],
                &[
                    0x02, 0xAB, 0xCD, 0xEF, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34,
                ],
            ],
        );
        check_payload(
            Some(36),
            &[
                &[0x02, 0xAB, 0xCD, 0xEF, 36, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                &[
                    0x02, 0xAB, 0xCD, 0xEF, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
                ],
                &[
                    0x02, 0xAB, 0xCD, 0xEF, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34,
                ],
                &[0x02, 0xAB, 0xCD, 0xEF, 35],
            ],
        );
    }

    #[test]
    fn display_works() {
        let header = Header::new(Kind::RequestPl, ChannelId(1), Id(2));
        let preamble = Preamble::new(header, Varint32::encode(678));

        assert_eq!(preamble.to_string(), "[RequestPl chan: 1 id: 2] [l=678]");

        let preamble_no_payload = Preamble::new(header, Varint32::SENTINEL);

        assert_eq!(preamble_no_payload.to_string(), "[RequestPl chan: 1 id: 2]");

        let msg = OutgoingMessage::new(header, Some(Bytes::from(&b"asdf"[..])));
        let (frame, _) = msg.frames().next_owned(4096);

        assert_eq!(
            frame.to_string(),
            "<[RequestPl chan: 1 id: 2] [l=4] 61 73 64 66 (4 bytes)>"
        );

        let msg_no_payload = OutgoingMessage::new(header, None);
        let (frame, _) = msg_no_payload.frames().next_owned(4096);

        assert_eq!(frame.to_string(), "<[RequestPl chan: 1 id: 2]>");
    }
}
