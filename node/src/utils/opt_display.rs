//! `Display` wrapper for optional values.
//!
//! Allows displaying an `Option<T>`, where `T` already implements `T`.

use std::fmt::{Display, Formatter, Result};

/// Wrapper around `Option` that implements `Display`.
pub struct OptDisplay<'a, 'b, T> {
    /// The actual `Option` being displayed.
    inner: Option<&'a T>,
    /// Value to substitute if `inner` is `None`.
    empty_display: &'b str,
}

impl<'a, 'b, T: Display> OptDisplay<'a, 'b, T> {
    /// Creates a new `OptDisplay`.
    #[inline]
    pub fn new(maybe_display: Option<&'a T>, empty_display: &'b str) -> Self {
        Self {
            inner: maybe_display,
            empty_display,
        }
    }
}

impl<'a, 'b, T: Display> Display for OptDisplay<'a, 'b, T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.inner {
            None => f.write_str(self.empty_display),
            Some(val) => val.fmt(f),
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
