//! Various utilities.
//!
//! The Generic functions that are not limited to a particular module, but are too small to warrant
//! being factored out into standalone crates.

pub mod round_robin;
pub mod zero_one_many;

/// Leak a value.
///
/// Moves a value to the heap and then forgets about, leaving only a static reference behind.
#[inline]
pub fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}
