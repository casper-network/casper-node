//! Various utilities.
//!
//! The Generic functions that are not limited to a particular module, but are too small to warrant
//! being factored out into standalone crates.

use std::fmt;
pub mod round_robin;
use std::cell;

/// Leak a value.
///
/// Moves a value to the heap and then forgets about, leaving only a static reference behind.
#[inline]
pub fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

/// Small amount store.
///
/// Stored in a smallvec to avoid allocations in case there are less than three items grouped. The
/// size of two items is chosen because one item is the most common use case, and large items are
/// typically boxed. In  the latter case two pointers and one enum variant discriminator is almost
/// the same size as an empty vec, which is two pointers.
pub type Multiple<T> = smallvec::SmallVec<[T; 2]>;

/// A display-helper that shows iterators display joined by ",".
#[derive(Debug)]
pub struct DisplayIter<T>(cell::RefCell<Option<T>>);

impl<T> DisplayIter<T> {
    pub fn new(item: T) -> Self {
        DisplayIter(cell::RefCell::new(Some(item)))
    }
}

impl<I, T> fmt::Display for DisplayIter<I>
where
    I: IntoIterator<Item = T>,
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
