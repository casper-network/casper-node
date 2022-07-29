use std::fmt::{Display, Formatter, Result};

pub struct OptDisplay<'a, 'b, T> {
    inner: Option<&'a T>,
    empty_display: &'b str,
}

impl<'a, 'b, T: Display> OptDisplay<'a, 'b, T> {
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
