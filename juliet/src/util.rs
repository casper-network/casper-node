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
    pub(crate) const fn new(buffer: &'a BytesMut, index: usize) -> Self {
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

#[cfg(test)]
mod tests {
    use bytes::{Bytes, BytesMut};
    use proptest_attr_macro::proptest;

    use crate::util::PayloadFormat;

    use super::Index;

    #[proptest]
    fn index_derefs_correctly(idx: usize) {
        let buffer = BytesMut::new();
        let index = Index::new(&buffer, idx);

        assert_eq!(*index, idx);
    }

    #[test]
    fn payload_formatting_works() {
        let payload_small = Bytes::from_static(b"hello");
        assert_eq!(
            PayloadFormat(&payload_small).to_string(),
            "68 65 6c 6c 6f (5 bytes)"
        );

        let payload_large = Bytes::from_static(b"goodbye, cruel world");
        assert_eq!(
            PayloadFormat(&payload_large).to_string(),
            "67 6f 6f 64 62 79 65 2c 20 63 72 75 65 6c 20 77 ... (20 bytes)"
        );

        let payload_empty = Bytes::from_static(b"");
        assert_eq!(PayloadFormat(&payload_empty).to_string(), "(0 bytes)");
    }
}
