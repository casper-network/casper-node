//! Implementation of scoped guards for various tasks.
use std::{cell::RefCell, rc::Rc};

use num::{CheckedAdd, CheckedSub, One, Zero};

/// A generic counting flag used by the [`ScopedCountingGuard`].
#[derive(Default, Clone)]
pub struct ScopedCounter<T>(Rc<RefCell<T>>);

impl<T: PartialOrd + Copy + Zero> ScopedCounter<T> {
    pub(crate) fn count(&self) -> T {
        *self.0.borrow()
    }

    pub(crate) fn is_set(&self) -> bool {
        self.count() > T::zero()
    }
}

impl<T: CheckedSub<Output = T> + CheckedAdd<Output = T> + One + Copy> ScopedCounter<T> {
    /// Increments counter and returns previous value.
    ///
    /// Returns None in case of overflow.
    fn increase(&self) -> Option<T> {
        let mut value = self.0.borrow_mut();
        let old_value = *value;
        *value = value.checked_add(&T::one())?;
        Some(old_value)
    }

    /// Decreases internal counter and returns previous value.
    fn decrease(&self) -> Option<T> {
        let mut value = self.0.borrow_mut();
        let old_value = *value;
        *value = value.checked_sub(&T::one())?;
        Some(old_value)
    }
}

/// When created it increases a counter, and decreases it when dropped.
/// Counter should never be manipulated manually
pub(crate) struct ScopedCountingGuard<T>
where
    T: CheckedAdd<Output = T> + CheckedSub<Output = T> + One + Copy,
{
    shared_value: ScopedCounter<T>,
}

impl<T> ScopedCountingGuard<T>
where
    T: CheckedAdd<Output = T> + CheckedSub<Output = T> + One + Copy,
{
    #[must_use]
    pub(crate) fn enter(shared_value: &ScopedCounter<T>) -> Option<Self> {
        let shared_value = shared_value.clone();
        shared_value.increase()?;
        Some(Self { shared_value })
    }

    fn exit(&mut self) {
        // SAFETY: Types are constrained and call to `exit` occurs only when Drop is called which
        // implies counter was increased before.
        self.shared_value.decrease().expect("assumed to never fail");
    }
}

impl<T> Drop for ScopedCountingGuard<T>
where
    T: CheckedAdd<Output = T> + CheckedSub<Output = T> + One + Copy,
{
    fn drop(&mut self) {
        self.exit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoping_test() {
        let counter = ScopedCounter::<i64>::default();

        {
            assert_eq!(counter.count(), 0);

            let _guard = ScopedCountingGuard::enter(&counter).expect("should enter");

            {
                assert_eq!(counter.count(), 1);

                let _guard = ScopedCountingGuard::enter(&counter).expect("should enter");

                assert_eq!(counter.count(), 2);
            }

            {
                let passed_counter = counter.clone();

                assert_eq!(passed_counter.count(), 1);

                let guard = ScopedCountingGuard::enter(&passed_counter).expect("should enter");

                let _moved_guard = guard;

                assert_eq!(passed_counter.count(), 2);
            }

            assert_eq!(counter.count(), 1);
        }

        assert_eq!(counter.count(), 0);
    }
}
