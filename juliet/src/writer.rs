use std::{collections::HashSet, io::Cursor};

use bytemuck::{Pod, Zeroable};
use bytes::{buf::Chain, Buf, Bytes};
use thiserror::Error;

use crate::{header::Header, varint::Varint32, ChannelConfiguration, ChannelId, Id};

pub struct OutgoingMessage {
    header: Header,
    payload: Option<Bytes>,
}

impl OutgoingMessage {
    fn frames<'a>(&'a self) -> FrameIter<'a> {
        FrameIter {
            msg: self,
            bytes_processed: 0,
        }
    }
}

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

    #[inline]
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

#[derive(Copy, Clone, Debug, Error)]
pub enum LocalProtocolViolation {
    /// TODO: docs with hint what the programming error could be
    #[error("sending would exceed request limit")]
    WouldExceedRequestLimit,
    /// TODO: docs with hint what the programming error could be
    #[error("invalid channel")]
    InvalidChannel(ChannelId),
}

impl<const N: usize> MessageWriteTracker<N> {
    #[inline(always)]
    fn channel_index(&self, channel: ChannelId) -> Result<usize, LocalProtocolViolation> {
        if channel.0 as usize >= N {
            Err(LocalProtocolViolation::InvalidChannel(channel))
        } else {
            Ok(channel.0 as usize)
        }
    }

    /// Returns whether or not it is permissible to send another request on given channel.
    #[inline]
    pub fn allowed_to_send_request(
        &self,
        channel: ChannelId,
    ) -> Result<bool, LocalProtocolViolation> {
        let chan_idx = self.channel_index(channel)?;
        let chan = &self.channels[chan_idx];

        Ok(chan.outgoing_request_ids.len() < chan.config.request_limit as usize)
    }

    /// Creates a new request to be sent.
    ///
    /// # Note
    ///
    /// Any caller of this functions should call `allowed_to_send_request()` before this function
    /// to ensure the channels request limit is not exceeded. Failure to do so may result in the
    /// peer closing the connection due to a protocol violation.
    pub fn create_request(
        &mut self,
        channel: ChannelId,
        payload: Option<Bytes>,
    ) -> Result<OutgoingMessage, LocalProtocolViolation> {
        let id = self.generate_id(channel);

        if !self.allowed_to_send_request(channel)? {
            return Err(LocalProtocolViolation::WouldExceedRequestLimit);
        }

        if let Some(payload) = payload {
            let header = Header::new(crate::header::Kind::RequestPl, channel, id);
            Ok(OutgoingMessage {
                header,
                payload: Some(payload),
            })
        } else {
            let header = Header::new(crate::header::Kind::Request, channel, id);
            Ok(OutgoingMessage {
                header,
                payload: None,
            })
        }
    }

    fn generate_id(&mut self, channel: ChannelId) -> Id {
        todo!()
    }
}
