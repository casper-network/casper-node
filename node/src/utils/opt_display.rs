//! `Display` wrapper for optional values.
//!
//! Allows displaying an `Option<T>`, where `T` already implements `Display`.

use std::fmt::{Display, Formatter, Result};

use serde::Serialize;

/// Wrapper around `Option` that implements `Display`.
///
/// For convenience, it also includes a `Serialize` implementation that works identical to the
/// underlying `Option<T>` serialization.
pub struct OptDisplay<'a, T> {
    /// The actual `Option` being displayed.
    inner: Option<T>,
    /// Value to substitute if `inner` is `None`.
    empty_display: &'a str,
}

impl<'a, T> Serialize for OptDisplay<'a, T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.inner.serialize(serializer)
    }
}

impl<'a, T: Display> OptDisplay<'a, T> {
    /// Creates a new `OptDisplay`.
    #[inline]
    pub fn new(maybe_display: Option<T>, empty_display: &'a str) -> Self {
        Self {
            inner: maybe_display,
            empty_display,
        }
    }
}

impl<'a, T: Display> Display for OptDisplay<'a, T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.inner {
            None => f.write_str(self.empty_display),
            Some(ref val) => val.fmt(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OptDisplay;

    #[test]
    fn opt_display_works() {
        let some_value: Option<u32> = Some(12345);

        assert_eq!(
            OptDisplay::new(some_value.as_ref(), "does not matter").to_string(),
            "12345"
        );

        let none_value: Option<u32> = None;
        assert_eq!(
            OptDisplay::new(none_value.as_ref(), "should be none").to_string(),
            "should be none"
        );
    }
}
