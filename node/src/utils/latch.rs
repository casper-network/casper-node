use std::ops::Deref;

use datasize::DataSize;

/// A wrapper around a value which only allows the value to be changed once.
#[derive(DataSize, Debug)]
pub(crate) struct Latch<T> {
    /// `true` if the value has already been reset.
    set: bool,
    /// The inner value.
    value: T,
}

impl<T> Latch<T> {
    /// Creates a new instance wrapping `value`.
    pub(crate) fn new(value: T) -> Self {
        Self { value, set: false }
    }

    /// Resets the inner value to `value`, disallowing further modifications in the future.
    ///
    /// Returns `false` if the value was already set.
    pub(crate) fn set(&mut self, value: T) -> bool {
        if self.set {
            return false;
        }
        self.set = true;
        self.value = value;
        true
    }
}

impl<T> Deref for Latch<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}
