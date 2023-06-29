use std::{collections::HashSet, io::Cursor};

use bytemuck::{Pod, Zeroable};
use bytes::{buf::Chain, Buf, Bytes};

use crate::{header::Header, varint::Varint32, ChannelConfiguration, Id};

#[must_use]
pub struct OutgoingMessage {
    header: Header,
    payload: Option<Bytes>,
}

impl OutgoingMessage {
    pub(super) fn new(header: Header, payload: Option<Bytes>) -> Self {
        Self { header, payload }
    }

    fn frames<'a>(&'a self) -> FrameIter<'a> {
        FrameIter {
            msg: self,
            bytes_processed: 0,
        }
    }
}

#[must_use]
struct FrameIter<'a> {
    msg: &'a OutgoingMessage,
    bytes_processed: usize,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct Preamble {
    header: Header,
    payload_length: Varint32,
}

impl Preamble {
    #[inline(always)]
    fn new(header: Header, payload_length: Varint32) -> Self {
        Self {
            header,
            payload_length,
        }
    }

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

impl<'a> FrameIter<'a> {
    fn next(&mut self, max_frame_size: usize) -> Option<OutgoingFrame> {
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
}

#[derive(Debug)]
#[repr(transparent)]
struct OutgoingFrame(Chain<Cursor<Preamble>, Bytes>);

impl OutgoingFrame {
    #[inline(always)]
    fn new(preamble: Preamble) -> Self {
        Self::new_with_payload(preamble, Bytes::new())
    }

    #[inline(always)]
    fn new_with_payload(preamble: Preamble, payload: Bytes) -> Self {
        OutgoingFrame(Cursor::new(preamble).chain(payload))
    }
}

pub struct Channel {
    config: ChannelConfiguration,
    outgoing_request_ids: HashSet<Id>,
}

pub struct MessageWriteTracker<const N: usize> {
    /// Outgoing channels
    channels: [Channel; N],
}
