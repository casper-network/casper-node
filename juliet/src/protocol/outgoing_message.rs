//! Outgoing message data.
//!
//! The [`protocol`](crate::protocol) module exposes a pure, non-IO state machine for handling the
//! juliet networking protocol, this module contains the necessary output types like
//! [`OutgoingMessage`].

use std::{
    fmt::{self, Debug, Display, Formatter, Write},
    io::Cursor,
    iter,
};

use bytemuck::{Pod, Zeroable};
use bytes::{buf::Chain, Buf, Bytes};

use crate::{header::Header, varint::Varint32};

use super::{payload_is_multi_frame, MaxFrameSize};

/// A message to be sent to the peer.
///
/// [`OutgoingMessage`]s are generated when the protocol requires data to be sent to the peer.
/// Unless the connection is terminated, they should not be dropped, but can be sent in any order.
///
/// A message that spans one or more frames must have its internal frame order preserved. In
/// general, the [`OutgoingMessage::frames()`] iterator should be used, even for single-frame
/// messages.
#[must_use]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutgoingMessage {
    /// The common header for all outgoing messages.
    header: Header,
    /// The payload, potentially split across multiple messages.
    payload: Option<Bytes>,
}

impl OutgoingMessage {
    /// Constructs a new outgoing message.
    // Note: Do not make this function available to users of the library, to avoid them constructing
    //       messages by accident that may violate the protocol.
    #[inline(always)]
    pub(super) const fn new(header: Header, payload: Option<Bytes>) -> Self {
        Self { header, payload }
    }

    /// Returns whether or not a message will span multiple frames.
    #[inline(always)]
    pub const fn is_multi_frame(&self, max_frame_size: MaxFrameSize) -> bool {
        if let Some(ref payload) = self.payload {
            payload_is_multi_frame(max_frame_size, payload.len())
        } else {
            false
        }
    }

    /// Creates an iterator over all frames in the message.
    #[inline(always)]
    pub const fn frames(self) -> FrameIter {
        FrameIter {
            msg: self,
            bytes_processed: 0,
        }
    }

    /// Creates an iterator over all frames in the message with a fixed maximum frame size.
    ///
    /// A slightly more convenient `frames` method, with a fixed `max_frame_size`. The resulting
    /// iterator will use slightly more memory than the equivalent `FrameIter`.
    pub fn frame_iter(self, max_frame_size: MaxFrameSize) -> impl Iterator<Item = OutgoingFrame> {
        let mut frames = Some(self.frames());

        iter::from_fn(move || {
            let iter = frames.take()?;
            let (frame, more) = iter.next_owned(max_frame_size);
            frames = more;
            Some(frame)
        })
    }

    /// Returns the outgoing message's header.
    #[inline(always)]
    pub const fn header(&self) -> Header {
        self.header
    }

    /// Calculates the total number of bytes that are not header data that will be transmitted with
    /// this message (the payload + its variable length encoded length prefix).
    #[inline]
    pub const fn non_header_len(&self) -> usize {
        match self.payload {
            Some(ref pl) => Varint32::length_of(pl.len() as u32) + pl.len(),
            None => 0,
        }
    }

    /// Calculates the number of frames this message will produce.
    #[inline]
    pub const fn num_frames(&self, max_frame_size: MaxFrameSize) -> usize {
        let usable_size = max_frame_size.without_header();

        let num_frames = (self.non_header_len() + usable_size - 1) / usable_size;
        if num_frames == 0 {
            1 // `Ord::max` is not `const fn`.
        } else {
            num_frames
        }
    }

    /// Calculates the total length in bytes of all frames produced by this message.
    #[inline]
    pub const fn total_len(&self, max_frame_size: MaxFrameSize) -> usize {
        self.num_frames(max_frame_size) * Header::SIZE + self.non_header_len()
    }

    /// Creates an byte-iterator over all frames in the message.
    ///
    /// The returned `ByteIter` will return all frames in sequence using the [`bytes::Buf`] trait,
    /// with no regard for frame boundaries, thus it is only suitable to send all frames of the
    /// message with no interleaved data.
    #[inline]
    pub fn iter_bytes(self, max_frame_size: MaxFrameSize) -> ByteIter {
        let length_prefix = self
            .payload
            .as_ref()
            .map(|pl| Varint32::encode(pl.len() as u32))
            .unwrap_or(Varint32::SENTINEL);
        ByteIter {
            msg: self,
            length_prefix,
            consumed: 0,
            max_frame_size,
        }
    }

    /// Writes out all frames as they should be sent out on the wire into a [`Bytes`] struct.
    ///
    /// Consider using the `frames()` or `bytes()` methods instead to avoid additional copies. This
    /// method is not zero-copy, but still consumes `self` to avoid a conversion of a potentially
    /// unshared payload buffer.
    #[inline]
    pub fn to_bytes(self, max_frame_size: MaxFrameSize) -> Bytes {
        let mut everything = self.iter_bytes(max_frame_size);
        everything.copy_to_bytes(everything.remaining())
    }
}

/// Combination of header and potential message payload length.
///
/// A message with a payload always starts with an initial frame that has a header and a varint
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
        Debug::fmt(&self.header, f)?;
        if !self.payload_length.is_sentinel() {
            write!(f, " [len={}]", self.payload_length.decode())?;
        }
        Ok(())
    }
}

impl Preamble {
    /// Creates a new preamble.
    ///
    /// Passing [`Varint32::SENTINEL`] as the length will cause it to be omitted.
    #[inline(always)]
    const fn new(header: Header, payload_length: Varint32) -> Self {
        Self {
            header,
            payload_length,
        }
    }

    /// Returns the length of the preamble when encoded as as a bytestring.
    #[inline(always)]
    const fn len(self) -> usize {
        Header::SIZE + self.payload_length.len()
    }

    #[inline(always)]
    const fn header(self) -> Header {
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
    /// Will return the next frame, and `Some(self)` if there are additional frames to send to
    /// complete the message, `None` otherwise.
    ///
    /// # Note
    ///
    /// While different [`OutgoingMessage`]s can have their send order mixed or interspersed, a
    /// caller MUST NOT send [`OutgoingFrame`]s of a single message in any order but the one
    /// produced by this method. In other words, reorder messages, but not frames within a message.
    pub fn next_owned(mut self, max_frame_size: MaxFrameSize) -> (OutgoingFrame, Option<Self>) {
        if let Some(ref payload) = self.msg.payload {
            let mut payload_remaining = payload.len() - self.bytes_processed;

            // If this is the first frame, include the message payload length.
            let length_prefix = if self.bytes_processed == 0 {
                Varint32::encode(payload_remaining as u32)
            } else {
                Varint32::SENTINEL
            };

            let preamble = Preamble::new(self.msg.header, length_prefix);

            let frame_capacity = max_frame_size.get_usize() - preamble.len();
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

    /// Returns the outgoing message's header.
    #[inline(always)]
    pub const fn header(&self) -> Header {
        self.msg.header()
    }
}

/// Byte-wise message iterator.
#[derive(Debug)]
pub struct ByteIter {
    /// The outgoing message.
    msg: OutgoingMessage,
    /// A written-out copy of the length prefixed.
    ///
    /// Handed out by reference.
    length_prefix: Varint32,
    /// Number of bytes already written/sent.
    // Note: The `ByteIter` uses `usize`s, since its primary use is to allow using the `Buf`
    //       interface, which can only deal with usize arguments anyway.
    consumed: usize,
    /// Maximum frame size at construction.
    max_frame_size: MaxFrameSize,
}

impl ByteIter {
    /// Returns the total number of bytes to be emitted by this [`ByteIter`].
    #[inline(always)]
    const fn total(&self) -> usize {
        self.msg.total_len(self.max_frame_size)
    }
}

impl Buf for ByteIter {
    #[inline(always)]
    fn remaining(&self) -> usize {
        self.total() - self.consumed
    }

    #[inline]
    fn chunk(&self) -> &[u8] {
        if self.remaining() == 0 {
            return &[];
        }

        // Determine where we are.
        let frames_completed = self.consumed / self.max_frame_size.get_usize();
        let frame_progress = self.consumed % self.max_frame_size.get_usize();
        let in_first_frame = frames_completed == 0;

        if frame_progress < Header::SIZE {
            // Currently sending the header.
            return &self.msg.header.as_ref()[frame_progress..];
        }

        debug_assert!(!self.length_prefix.is_sentinel());
        if in_first_frame && frame_progress < (Header::SIZE + self.length_prefix.len()) {
            // Currently sending the payload length prefix.
            let varint_progress = frame_progress - Header::SIZE;
            return &self.length_prefix.as_ref()[varint_progress..];
        }

        // Currently sending a payload chunk.
        let space_in_frame = self.max_frame_size.without_header();
        let first_preamble = Header::SIZE + self.length_prefix.len();
        let (frame_payload_start, frame_payload_progress, frame_payload_end) = if in_first_frame {
            (
                0,
                frame_progress - first_preamble,
                self.max_frame_size.get_usize() - first_preamble,
            )
        } else {
            let start = frames_completed * space_in_frame - self.length_prefix.len();
            (start, frame_progress - Header::SIZE, start + space_in_frame)
        };

        let current_frame_chunk = self
            .msg
            .payload
            .as_ref()
            .map(|pl| &pl[frame_payload_start..frame_payload_end.min(pl.remaining())])
            .unwrap_or_default();

        &current_frame_chunk[frame_payload_progress..]
    }

    #[inline(always)]
    fn advance(&mut self, cnt: usize) {
        self.consumed = (self.consumed + cnt).min(self.total());
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

    /// Writes out the frame.
    ///
    /// Equivalent to `self.copy_to_bytes(self.remaining)`.
    #[inline]
    pub fn to_bytes(mut self) -> Bytes {
        self.copy_to_bytes(self.remaining())
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

    use bytes::{Buf, Bytes};

    use crate::{
        header::{Header, Kind},
        protocol::MaxFrameSize,
        varint::Varint32,
        ChannelId, Id,
    };

    use super::{FrameIter, OutgoingMessage, Preamble};

    /// Maximum frame size used across tests.
    const MAX_FRAME_SIZE: MaxFrameSize = MaxFrameSize::new(16);

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
        assert_eq!(msg.clone().frames().header(), header);
        assert_eq!(expected.len() > 1, msg.is_multi_frame(MAX_FRAME_SIZE));
        assert_eq!(expected.len(), msg.num_frames(MAX_FRAME_SIZE));

        // Payload data check.
        if let Some(length) = length {
            assert_eq!(
                length + Varint32::length_of(length as u32),
                msg.non_header_len()
            );
        } else {
            assert_eq!(msg.non_header_len(), 0);
        }

        // A zero-byte payload is still expected to produce a single byte for the 0-length.
        let frames = collect_frames(msg.clone().frames());

        // Addtional test: Ensure `frame_iter` yields the same result.
        let mut from_frame_iter: Vec<u8> = Vec::new();
        for frame in msg.clone().frame_iter(MAX_FRAME_SIZE) {
            from_frame_iter.extend(frame.to_bytes());
        }

        // We could compare without creating a new vec, but this gives nicer error messages.
        let comparable: Vec<_> = frames.iter().map(|v| v.as_slice()).collect();
        assert_eq!(&comparable, expected);

        // Ensure that the written out version is the same as expected.
        let expected_bytestring: Vec<u8> =
            expected.iter().flat_map(Deref::deref).copied().collect();
        assert_eq!(expected_bytestring.len(), msg.total_len(MAX_FRAME_SIZE));
        assert_eq!(from_frame_iter, expected_bytestring);

        let mut bytes_iter = msg.clone().iter_bytes(MAX_FRAME_SIZE);
        let written_out = bytes_iter.copy_to_bytes(bytes_iter.remaining()).to_vec();
        assert_eq!(written_out, expected_bytestring);
        let converted_to_bytes = msg.clone().to_bytes(MAX_FRAME_SIZE);
        assert_eq!(converted_to_bytes, expected_bytestring);

        // Finally, we do a trickle-test with various step sizes.
        for step_size in 1..=(MAX_FRAME_SIZE.get_usize() * 2) {
            let mut buf: Vec<u8> = Vec::new();

            let mut bytes_iter = msg.clone().iter_bytes(MAX_FRAME_SIZE);

            while bytes_iter.remaining() > 0 {
                let chunk = bytes_iter.chunk();
                let next_step = chunk.len().min(step_size);
                buf.extend(&chunk[..next_step]);
                bytes_iter.advance(next_step);
            }

            assert_eq!(buf, expected_bytestring);
        }
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
    fn bytes_iterator_smoke_test() {
        let payload = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11][..];

        // Expected output:
        // &[0x02, 0xAB, 0xCD, 0xEF, 12, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        // &[0x02, 0xAB, 0xCD, 0xEF, 11],

        let msg = OutgoingMessage::new(
            Header::new(Kind::RequestPl, ChannelId(0xAB), Id(0xEFCD)),
            Some(Bytes::from(payload)),
        );

        let mut byte_iter = msg.iter_bytes(MAX_FRAME_SIZE);

        // First header.
        assert_eq!(byte_iter.remaining(), 21);
        assert_eq!(byte_iter.chunk(), &[0x02, 0xAB, 0xCD, 0xEF]);
        assert_eq!(byte_iter.chunk(), &[0x02, 0xAB, 0xCD, 0xEF]);
        byte_iter.advance(2);
        assert_eq!(byte_iter.remaining(), 19);
        assert_eq!(byte_iter.chunk(), &[0xCD, 0xEF]);
        byte_iter.advance(2);
        assert_eq!(byte_iter.remaining(), 17);

        // Varint encoding length.
        assert_eq!(byte_iter.chunk(), &[12]);
        byte_iter.advance(1);
        assert_eq!(byte_iter.remaining(), 16);

        // Payload of first frame (MAX_FRAME_SIZE - 5 = 11 bytes).
        assert_eq!(byte_iter.chunk(), &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        byte_iter.advance(1);
        assert_eq!(byte_iter.chunk(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        byte_iter.advance(5);
        assert_eq!(byte_iter.chunk(), &[6, 7, 8, 9, 10]);
        byte_iter.advance(5);

        // Second frame.
        assert_eq!(byte_iter.remaining(), 5);
        assert_eq!(byte_iter.chunk(), &[0x02, 0xAB, 0xCD, 0xEF]);
        byte_iter.advance(3);
        assert_eq!(byte_iter.chunk(), &[0xEF]);
        byte_iter.advance(1);
        assert_eq!(byte_iter.remaining(), 1);
        assert_eq!(byte_iter.chunk(), &[11]);
        byte_iter.advance(1);
        assert_eq!(byte_iter.remaining(), 0);
        assert_eq!(byte_iter.chunk(), &[0u8; 0]);
        assert_eq!(byte_iter.chunk(), &[0u8; 0]);
        assert_eq!(byte_iter.chunk(), &[0u8; 0]);
        assert_eq!(byte_iter.chunk(), &[0u8; 0]);
        assert_eq!(byte_iter.chunk(), &[0u8; 0]);
        assert_eq!(byte_iter.remaining(), 0);
        assert_eq!(byte_iter.remaining(), 0);
        assert_eq!(byte_iter.remaining(), 0);
        assert_eq!(byte_iter.remaining(), 0);
    }

    #[test]
    fn display_works() {
        let header = Header::new(Kind::RequestPl, ChannelId(1), Id(2));
        let preamble = Preamble::new(header, Varint32::encode(678));

        assert_eq!(preamble.to_string(), "[RequestPl chan: 1 id: 2] [len=678]");

        let preamble_no_payload = Preamble::new(header, Varint32::SENTINEL);

        assert_eq!(preamble_no_payload.to_string(), "[RequestPl chan: 1 id: 2]");

        let msg = OutgoingMessage::new(header, Some(Bytes::from(&b"asdf"[..])));
        let (frame, _) = msg.frames().next_owned(Default::default());

        assert_eq!(
            frame.to_string(),
            "<[RequestPl chan: 1 id: 2] [len=4] 61 73 64 66 (4 bytes)>"
        );

        let msg_no_payload = OutgoingMessage::new(header, None);
        let (frame, _) = msg_no_payload.frames().next_owned(Default::default());

        assert_eq!(frame.to_string(), "<[RequestPl chan: 1 id: 2]>");
    }
}
