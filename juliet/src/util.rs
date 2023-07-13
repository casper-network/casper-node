//! Miscellaneous utilities used across multiple modules.

use std::{marker::PhantomData, ops::Deref};

use bytes::BytesMut;

/// Bytes offset with a lifetime.
///
/// Helper type that ensures that offsets that are depending on a buffer are not being invalidated
/// through accidental modification.
pub(crate) struct Index<'a> {
    /// The byte offset this `Index` represents.
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
    /// Creates a new `Index` with offset value `index`, borrowing `buffer`.
    pub(crate) fn new(buffer: &'a BytesMut, index: usize) -> Self {
        let _ = buffer;
        Index {
            index,
            buffer: PhantomData,
        }
    }
}

#[cfg(feature = "tracing")]
pub mod tracing_support {
    //! Display helper for formatting messages in `tracing` log messages.
    use std::fmt::{self, Display, Formatter};

    use bytes::Bytes;

    /// Pretty prints a single payload.
    pub struct PayloadFormat<'a>(pub &'a Bytes);

    impl<'a> Display for PayloadFormat<'a> {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            let raw = self.0.as_ref();

            for &byte in &raw[0..raw.len().min(16)] {
                write!(f, "{:02x} ", byte)?;
            }

            if raw.len() > 16 {
                f.write_str("...")?;
            }

            write!(f, " ({} bytes)", raw.len())?;

            Ok(())
        }
    }

    /// Pretty prints an optional payload.
    pub struct OptPayloadFormat<'a>(pub Option<&'a Bytes>);

    impl<'a> Display for OptPayloadFormat<'a> {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            match self.0 {
                None => f.write_str("(no payload)"),
                Some(inner) => PayloadFormat(inner).fmt(f),
            }
        }
    }
}
