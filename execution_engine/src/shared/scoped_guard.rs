//! Implementation of scoped guards for various tasks.
use std::{cell::RefCell, rc::Rc};

/// When created it increases a counter, and decreases it when dropped.
pub(crate) struct ScopedCountingGuard {
    shared_value: Rc<RefCell<i32>>,
}

impl ScopedCountingGuard {
    #[must_use]
    pub(crate) fn new(shared_value: &Rc<RefCell<i32>>) -> Self {
        shared_value.replace_with(|&mut value| value + 1);
        let shared_value = Rc::clone(shared_value);
        Self { shared_value }
    }
}

impl Drop for ScopedCountingGuard {
    fn drop(&mut self) {
        *self.shared_value.borrow_mut() -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoping_test() {
        let value = Rc::new(RefCell::new(0i32));
        {
            assert_eq!(*value.borrow(), 0);

            let _guard = ScopedCountingGuard::new(&value);

            {
                assert_eq!(*value.borrow(), 1);

                let _guard = ScopedCountingGuard::new(&value);

                assert_eq!(*value.borrow(), 2);
            }

            assert_eq!(*value.borrow(), 1);
        }

        assert_eq!(*value.borrow(), 0);
    }
}
