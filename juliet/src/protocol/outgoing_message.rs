//! Outgoing message data.
//!
//! The [`protocol`](crate::protocol) module exposes a pure, non-IO state machine for handling the
//! juliet networking protocol, this module contains the necessary output types like
//! [`OutgoingMessage`].

use std::{io::Cursor, iter};

use bytemuck::{Pod, Zeroable};
use bytes::{buf::Chain, Buf, Bytes};

use crate::{header::Header, varint::Varint32};

/// A message to be sent to the peer.
///
/// [`OutgoingMessage`]s are generated when the protocol requires data to be sent to the peer.
/// Unless the connection is terminated, they should not be dropped, but can be sent in any order.
///
/// While *frames* can be sent in any order, a message may span one or more frames, which can be
/// interspersed with other messages at will. In general, the [`OutgoingMessage::frames()`] iterator
/// should be used, even for single-frame messages.
#[must_use]
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
    pub(super) fn new(header: Header, payload: Option<Bytes>) -> Self {
        Self { header, payload }
    }

    /// Creates an iterator over all frames in the message.
    pub fn frames(self) -> FrameIter {
        FrameIter {
            msg: self,
            bytes_processed: 0,
        }
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
    #[inline]
    fn len(&self) -> usize {
        Header::SIZE + self.payload_length.len()
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
    /// # Note
    ///
    /// While different [`OutgoingMessage`]s can have their send order mixed or interspersed, a
    /// caller MUST NOT send [`OutgoingFrame`]s in any order but the one produced by this method.
    /// In other words, reorder messages, but not frames within a message.
    pub fn next(&mut self, max_frame_size: usize) -> Option<OutgoingFrame> {
        if let Some(ref payload) = self.msg.payload {
            let payload_remaining = payload.len() - self.bytes_processed;

            if payload_remaining == 0 {
                return None;
            }

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

            let frame_capacity = max_frame_size - preamble.len();
            let frame_payload_len = frame_capacity.min(payload_remaining);

            let range = self.bytes_processed..(self.bytes_processed + frame_payload_len);
            let frame_payload = payload.slice(range);
            self.bytes_processed += frame_payload_len;

            Some(OutgoingFrame::new_with_payload(preamble, frame_payload))
        } else {
            if self.bytes_processed == 0 {
                self.bytes_processed = usize::MAX;
                return Some(OutgoingFrame::new(Preamble::new(
                    self.msg.header,
                    Varint32::SENTINEL,
                )));
            } else {
                return None;
            }
        }
    }

    /// Returns a [`std::iter::Iterator`] implementing frame iterator.
    #[inline]
    pub fn into_iter(mut self, max_frame_size: usize) -> impl Iterator<Item = OutgoingFrame> {
        iter::from_fn(move || self.next(max_frame_size))
    }
}

/// A single frame to be sent.
///
/// An [`OutgoingFrame`] implements [`bytes::Buf`], which will yield the bytes necessary to send it
/// across the wire to a peer.
#[derive(Debug)]
#[repr(transparent)]
#[must_use]
pub struct OutgoingFrame(Chain<Cursor<Preamble>, Bytes>);

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
    /// Panics in debug mode if [`Preamble`] does not have a correct payload length.
    #[inline(always)]
    fn new_with_payload(preamble: Preamble, payload: Bytes) -> Self {
        debug_assert!(
            !preamble.payload_length.is_sentinel() || (payload.len() == 0),
            "frames without a payload must not contain a preamble with a payload length"
        );

        debug_assert!(
            preamble.payload_length.is_sentinel()
                || preamble.payload_length.decode() as usize == payload.len(),
            "frames with a payload must have a matching decoded payload length"
        );

        OutgoingFrame(Cursor::new(preamble).chain(payload))
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
