//! Frame encoding/decoding.
//!
//! A frame is a finite unit of bytes to be sent discretely over an underlying networking stream.
//! Usually some sort of framing mechanism needs to be employed to convert from discrete values to
//! continuous bytestreams and back, see the [`FrameEncoder`] and [`FrameDecoder`] traits for
//! details.
//!
//! # Available implementations
//!
//! Currently, the following transcoders and frame decoders are available:
//!
//! * [`length_delimited`]: Transforms byte-like values into self-contained frames with a
//!   length-prefix.

pub mod length_delimited;

use std::fmt::Debug;

use bytes::{Buf, Bytes, BytesMut};
use thiserror::Error;

/// Frame decoder.
///
/// A frame decoder extracts a frame from a continous bytestream.
pub trait FrameDecoder {
    /// Decoding error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Decodes a frame from a buffer.
    ///
    /// Produces either a frame, an error or an indicator for incompletion. See [`DecodeResult`] for
    /// details.
    ///
    /// Implementers of this function are expected to remove completed frames from `buffer`.
    fn decode_frame(&mut self, buffer: &mut BytesMut) -> DecodeResult<Bytes, Self::Error>;
}

/// Frame encoder.
///
/// A frame encoder encodes a frame into a representation suitable for writing to a bytestream.
pub trait FrameEncoder<T>
where
    T: Buf,
{
    /// Encoding error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// The output containing an encoded frame.
    type Output: Buf + Send;

    /// Encodes a given frame into a sendable representation.
    fn encode_frame(&mut self, buffer: T) -> Result<Self::Output, Self::Error>;
}

/// The outcome of a frame decoding operation.
#[derive(Debug, Error)]
pub enum DecodeResult<T, E> {
    /// A complete item was decoded.
    Item(T),
    /// No frame could be decoded, an unknown amount of bytes is still required.
    Incomplete,
    /// No frame could be decoded, but the remaining amount of bytes required is known.
    Remaining(usize),
    /// Irrecoverably failed to decode frame.
    Failed(E),
}
