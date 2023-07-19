//! Miscellaneous utilities used across multiple modules.

use std::{
    fmt::{self, Display, Formatter},
    marker::PhantomData,
    ops::Deref,
};

use bytes::{Bytes, BytesMut};

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

/// Pretty prints a single payload.
pub(crate) struct PayloadFormat<'a>(pub &'a Bytes);

impl<'a> Display for PayloadFormat<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let raw = self.0.as_ref();

        for &byte in &raw[0..raw.len().min(16)] {
            write!(f, "{:02x} ", byte)?;
        }

        if raw.len() > 16 {
            f.write_str("... ")?;
        }

        write!(f, "({} bytes)", raw.len())?;

        Ok(())
    }
}
