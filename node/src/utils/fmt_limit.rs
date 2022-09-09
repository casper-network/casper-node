//! Wrappers to display a limited amount of data from collections using `fmt`.

use std::fmt::{self, Debug, Formatter, Write};

/// A display wrapper showing a limited amount of a formatted rendering.
///
/// Any characters exceeding the given length will be omitted and replaced by `...`.
pub(crate) struct FmtLimit<'a, T> {
    limit: usize,
    item: &'a T,
}

impl<'a, T> FmtLimit<'a, T> {
    #[inline]

    /// Creates a new limited formatter.
    pub(crate) fn new(limit: usize, item: &'a T) -> Self {
        FmtLimit { limit, item }
    }
}

/// Helper that limits writing to a given `fmt::Writer`.
struct LimitWriter<'a, W> {
    /// The wrapper writer.
    inner: &'a mut W,
    /// How many characters are left.
    left: usize,
    /// Whether or not the writer is "closed".
    ///
    /// Closing happens when an additional character is written after `left` has reached 0 and will
    /// trigger the ellipses to be written out.
    closed: bool,
}

impl<'a, W> LimitWriter<'a, W> {
    /// Constructs a new `LimitWriter`.
    #[inline]
    fn new(inner: &'a mut W, limit: usize) -> Self {
        LimitWriter {
            inner,
            left: limit,
            closed: false,
        }
    }
}

impl<'a, W> Write for LimitWriter<'a, W>
where
    W: Write,
{
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.closed {
            return Ok(());
        }

        if self.left == 0 {
            self.closed = true;
            self.inner.write_str("...")?;
            return Ok(());
        }

        // A tad bit slow, but required for correct unicode output.
        for c in s.chars().take(self.left) {
            self.write_char(c)?;
        }

        Ok(())
    }

    #[inline]
    fn write_char(&mut self, c: char) -> fmt::Result {
        if self.closed {
            return Ok(());
        }

        if self.left == 0 {
            self.closed = true;
            self.inner.write_str("...")?;
            return Ok(());
        }

        self.left -= 1;
        self.inner.write_char(c)
    }
}

impl<'a, T> Debug for FmtLimit<'a, T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut limit_writer = LimitWriter::new(f, self.limit);
        write!(&mut limit_writer, "{:?}", self.item)
    }
}

// Note: If required, a `Display` implementation can be added easily for `FmtLimit`.

#[cfg(test)]
mod tests {
    use crate::utils::fmt_limit::FmtLimit;

    #[test]
    fn limit_debug_works() {
        let collection: Vec<_> = (0..5).into_iter().collect();

        // Sanity check.
        assert_eq!(format!("{:?}", collection), "[0, 1, 2, 3, 4]");

        assert_eq!(format!("{:?}", FmtLimit::new(3, &collection)), "[0,...");
        assert_eq!(format!("{:?}", FmtLimit::new(0, &collection)), "...");
        assert_eq!(
            format!("{:?}", FmtLimit::new(1000, &collection)),
            "[0, 1, 2, 3, 4]"
        );
        assert_eq!(
            format!("{:?}", FmtLimit::new(15, &collection)),
            "[0, 1, 2, 3, 4]"
        );
    }
}
