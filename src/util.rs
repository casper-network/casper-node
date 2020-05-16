//! Various utilities.
//!
//! The Generic functions that are not limited to a particular module, but are too small to warrant
//! being factored out into standalone crates.

pub mod round_robin;

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
