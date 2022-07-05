use std::marker::PhantomData;

use bytes::Buf;
use futures::Sink;

/// Encoder.
///
/// An encoder takes a value of one kind and transforms it to another.
pub trait Encoder<F> {
    /// Encoding error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// The wrapped frame resulting from encoding the given raw frame.
    ///
    /// While this can be simply `Bytes`, using something like `bytes::Chain` allows for more
    /// efficient encoding here.
    type Output: Buf + Send + Sync + 'static;

    /// Encode a value.
    ///
    /// The resulting `Bytes` should be the bytes to send into the outgoing stream, it must contain
    /// the information required for an accompanying `Decoder` to be able to reconstruct the frame
    /// from a raw byte stream.
    fn encode(&mut self, input: F) -> Result<Self::Output, Self::Error>;
}

struct EncodingAdapter<E, F> {
    encoder: E,
    _phantom: PhantomData<F>,
}

impl<E, F> Sink<F> for EncodingAdapter<E, F> {}
