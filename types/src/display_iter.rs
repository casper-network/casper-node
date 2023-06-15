use core::{
    cell::RefCell,
    fmt::{self, Display, Formatter},
};

/// A helper to allow `Display` printing the items of an iterator with a comma and space between
/// each.
#[derive(Debug)]
pub struct DisplayIter<T>(RefCell<Option<T>>);

impl<T> DisplayIter<T> {
    /// Returns a new `DisplayIter`.
    pub fn new(item: T) -> Self {
        DisplayIter(RefCell::new(Some(item)))
    }
}

impl<I, T> Display for DisplayIter<I>
where
    I: IntoIterator<Item = T>,
    T: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(src) = self.0.borrow_mut().take() {
            let mut first = true;
            for item in src.into_iter().take(f.width().unwrap_or(usize::MAX)) {
                if first {
                    first = false;
                    write!(f, "{}", item)?;
                } else {
                    write!(f, ", {}", item)?;
                }
            }

            Ok(())
        } else {
            write!(f, "DisplayIter:GONE")
        }
    }
}
