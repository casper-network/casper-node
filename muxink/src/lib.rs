//! Asynchronous multiplexing

pub mod backpressured;
pub mod codec;
pub mod error;
pub mod fragmented;
pub mod io;
pub mod mux;
#[cfg(test)]
pub mod testing;

use std::num::NonZeroUsize;

use bytes::Buf;
use codec::{
    length_delimited::{LengthDelimited, LengthPrefixedFrame},
    ResultTranscoder, Transcoder, TranscodingSink, TranscodingStream,
};
use fragmented::{Defragmentizer, Fragmentizer, SingleFragment};
use futures::{Sink, Stream};

/// Helper macro for returning a `Poll::Ready(Err)` eagerly.
///
/// Can be remove once `Try` is stabilized for `Poll`.
#[macro_export]
macro_rules! try_ready {
    ($ex:expr) => {
        match $ex {
            Err(e) => return Poll::Ready(Err(e.into())),
            Ok(v) => v,
        }
    };
}

/// A frame for stack allocated data.
#[derive(Debug)]
pub struct ImmediateFrame<A> {
    /// How much of the frame has been read.
    pos: usize,
    /// The actual value contained.
    value: A,
}

impl<A> ImmediateFrame<A> {
    #[inline]
    pub fn new(value: A) -> Self {
        Self { pos: 0, value }
    }
}

/// Implements conversion functions to immediate types for atomics like `u8`, etc.
macro_rules! impl_immediate_frame_le {
    ($t:ty) => {
        impl From<$t> for ImmediateFrame<[u8; ::std::mem::size_of::<$t>()]> {
            #[inline]
            fn from(value: $t) -> Self {
                ImmediateFrame::new(value.to_le_bytes())
            }
        }
    };
}

impl_immediate_frame_le!(u8);
impl_immediate_frame_le!(u16);
impl_immediate_frame_le!(u32);

impl<A> Buf for ImmediateFrame<A>
where
    A: AsRef<[u8]>,
{
    fn remaining(&self) -> usize {
        // Does not overflow, as `pos` is  `< .len()`.

        self.value.as_ref().len() - self.pos
    }

    fn chunk(&self) -> &[u8] {
        // Safe access, as `pos` is guaranteed to be `< .len()`.
        &self.value.as_ref()[self.pos..]
    }

    fn advance(&mut self, cnt: usize) {
        // This is the only function modifying `pos`, upholding the invariant of it being smaller
        // than the length of the data we have.
        self.pos = (self.pos + cnt).min(self.value.as_ref().len());
    }
}

/// Convenience trait for construction of sink chains.
pub trait SinkMuxExt: Sized {
    /// Wraps the current sink in a transcoder.
    ///
    /// The resulting sink will pass all items through the given transcoder before passing them on.
    fn with_transcoder<Input, T, NewInput>(
        self,
        transcoder: T,
    ) -> TranscodingSink<T, NewInput, Self>
    where
        T: Transcoder<NewInput, Output = Input>;

    /// Wraps the current sink in a bincode transcoder.
    #[cfg(feature = "bincode")]
    fn bincode<T>(self) -> TranscodingSink<codec::bincode::BincodeEncoder<T>, T, Self>
    where
        Self: Sink<bytes::Bytes>,
        T: serde::Serialize + Sync + Send + 'static,
    {
        self.with_transcoder(codec::bincode::BincodeEncoder::new())
    }

    /// Wraps the current sink in a fragmentizer.
    fn fragmenting<F>(self, fragment_size: NonZeroUsize) -> Fragmentizer<Self, F>
    where
        Self: Sink<SingleFragment> + Unpin,
        F: Buf + Send + Sync + 'static;

    /// Wrap current sink in length delimination.
    ///
    /// Equivalent to `.with_transcoder(LengthDelimited)`.
    fn length_delimited<F>(self) -> TranscodingSink<LengthDelimited, F, Self>
    where
        Self: Sink<LengthPrefixedFrame<F>>,
        F: Buf + Send + Sync + 'static,
    {
        self.with_transcoder(LengthDelimited)
    }
}

impl<S> SinkMuxExt for S {
    fn with_transcoder<Input, T, NewInput>(
        self,
        transcoder: T,
    ) -> TranscodingSink<T, NewInput, Self> {
        TranscodingSink::new(transcoder, self)
    }

    fn fragmenting<F>(self, fragment_size: NonZeroUsize) -> Fragmentizer<Self, F>
    where
        Self: Sink<SingleFragment> + Unpin,
        F: Buf + Send + Sync + 'static,
    {
        Fragmentizer::new(fragment_size, self)
    }
}

/// Convenience trait for the construction of stream chains.
// Note: The trait bounds are not strictly necessary, but make compiler error messages a lot easier
//       to read.
pub trait StreamMuxExt: Sized + Stream + Unpin {
    /// Wraps the current stream with a transcoder.
    fn with_transcoder<T>(self, transcoder: T) -> TranscodingStream<T, Self>
    where
        T: Transcoder<Self::Item> + Unpin,
    {
        TranscodingStream::new(transcoder, self)
    }

    /// Wraps the current stream with a `Result`-mapping transcoder.
    #[inline]
    fn and_then_transcode<Trans, E, T>(
        self,
        transcoder: Trans,
    ) -> TranscodingStream<ResultTranscoder<Trans, E>, Self>
    where
        Trans: Transcoder<T>,
        Self: Stream<Item = Result<T, E>>,
    {
        let result_transcoder = ResultTranscoder::<_, E>::new(transcoder);
        TranscodingStream::new(result_transcoder, self)
    }

    /// Wraps the current stream in a defragmentizer.
    fn defragmenting(self, max_frame_size: usize) -> Defragmentizer<Self> {
        Defragmentizer::new(max_frame_size, self)
    }
}

impl<S> StreamMuxExt for S where S: Sized + Stream + Unpin {}

#[rustfmt::skip]
#[cfg(test)]
pub(crate) mod tests {

    // /// Test an "end-to-end" instance of the assembled pipeline for sending.
    // #[test]
    // fn fragmented_length_prefixed_sink() {
    //     let (tx, rx) = pipe();

    //     let frame_writer = FrameWriter::new(LengthDelimited, tx);
    //     let mut fragmented_sink =
    //         make_fragmentizer::<_, Infallible>(frame_writer, NonZeroUsize::new(5).unwrap());

    //     let frame_reader = FrameReader::new(LengthDelimited, rx, TESTING_BUFFER_INCREMENT);
    //     let fragmented_reader = make_defragmentizer(frame_reader);

    //     let sample_data = Bytes::from(&b"QRSTUV"[..]);

    //     fragmented_sink
    //         .send(sample_data)
    //         .now_or_never()
    //         .unwrap()
    //         .expect("send failed");

    //     // Drop the sink, to ensure it is closed.
    //     drop(fragmented_sink);

    //     let round_tripped: Vec<_> = fragmented_reader.collect().now_or_never().unwrap();

    //     assert_eq!(round_tripped, &[&b"QRSTUV"[..]])
    // }

    // #[test]
    // fn from_bytestream_to_frame() {
    //     let input = &b"\x06\x00\x00ABCDE\x06\x00\x00FGHIJ\x03\x00\xffKL"[..];
    //     let expected = "ABCDEFGHIJKL";

    //     let defragmentizer = make_defragmentizer(FrameReader::new(
    //         LengthDelimited,
    //         input,
    //         TESTING_BUFFER_INCREMENT,
    //     ));

    //     let messages: Vec<_> = defragmentizer.collect().now_or_never().unwrap();
    //     assert_eq!(
    //         expected,
    //         messages.first().expect("should have at least one message")
    //     );
    // }

    // #[test]
    // fn from_bytestream_to_multiple_frames() {
    //     let input = &b"\x06\x00\x00ABCDE\x06\x00\x00FGHIJ\x03\x00\xffKL\x10\x00\xffSINGLE_FRAGMENT\x02\x00\x00C\x02\x00\x00R\x02\x00\x00U\x02\x00\x00M\x02\x00\x00B\x02\x00\xffS"[..];
    //     let expected: &[&[u8]] = &[b"ABCDEFGHIJKL", b"SINGLE_FRAGMENT", b"CRUMBS"];

    //     let defragmentizer = make_defragmentizer(FrameReader::new(
    //         LengthDelimited,
    //         input,
    //         TESTING_BUFFER_INCREMENT,
    //     ));

    //     let messages: Vec<_> = defragmentizer.collect().now_or_never().unwrap();
    //     assert_eq!(expected, messages);
    // }

    // #[test]
    // fn ext_decorator_encoding() {
    //     let mut sink: TranscodingSink<
    //         LengthDelimited,
    //         Bytes,
    //         TranscodingSink<LengthDelimited, LengthPrefixedFrame<Bytes>, TestingSink>,
    //     > = TranscodingSink::new(
    //         LengthDelimited,
    //         TranscodingSink::new(LengthDelimited, TestingSink::new()),
    //     );

    //     let inner: TranscodingSink<LengthDelimited, Bytes, TestingSink> =
    //         TestingSink::new().with_transcoder(LengthDelimited);

    //     let mut sink2: TranscodingSink<
    //         LengthDelimited,
    //         Bytes,
    //         TranscodingSink<LengthDelimited, LengthPrefixedFrame<Bytes>, TestingSink>,
    //     > = SinkMuxExt::<LengthPrefixedFrame<Bytes>>::with_transcoder(inner, LengthDelimited);

    //     sink.send(Bytes::new()).now_or_never();
    // }

    // struct StrLen;

    // impl Transcoder<String> for StrLen {
    //     type Error = Infallible;

    //     type Output = [u8; 4];

    //     fn transcode(&mut self, input: String) -> Result<Self::Output, Self::Error> {
    //         Ok((input.len() as u32).to_le_bytes())
    //     }
    // }

    // struct BytesEnc;

    // impl<U> Transcoder<U> for BytesEnc
    // where
    //     U: AsRef<[u8]>,
    // {
    //     type Error = Infallible;

    //     type Output = Bytes;

    //     fn transcode(&mut self, input: U) -> Result<Self::Output, Self::Error> {
    //         Ok(Bytes::copy_from_slice(input.as_ref()))
    //     }
    // }

    // #[test]
    // fn ext_decorator_encoding() {
    //     let sink = TranscodingSink::new(LengthDelimited, TestingSink::new());
    //     let mut outer_sink = TranscodingSink::new(StrLen, TranscodingSink::new(BytesEnc, sink));

    //     outer_sink
    //         .send("xx".to_owned())
    //         .now_or_never()
    //         .unwrap()
    //         .unwrap();

    //     let mut sink2 = TestingSink::new()
    //         .length_delimited()
    //         .with_transcoder(BytesEnc)
    //         .with_transcoder(StrLen);

    //     sink2.send("xx".to_owned()).now_or_never().unwrap().unwrap();
    // }
}
