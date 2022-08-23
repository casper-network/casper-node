//! Wrappers to display a limited amount of data from collections using `fmt`.

use std::fmt::{self, Debug, Formatter};

/// A display wrapper showing a limited amount of items in a slice.
pub(crate) struct LimitSlice<'a, T, const N: usize = 10>(&'a [T]);

impl<'a, T, const N: usize> LimitSlice<'a, T, N> {
    /// Creates a new limited slice instance.
    pub(crate) fn new(slice: &'a [T]) -> Self {
        LimitSlice(slice)
    }
}

impl<'a, T, const N: usize> Debug for LimitSlice<'a, T, N>
where
    T: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;

        let count = N.min(self.0.len());
        for (idx, item) in self.0.iter().take(count).enumerate() {
            Debug::fmt(item, f)?;
            if idx + 1 != count {
                f.write_str(", ")?;
            }
        }

        if count != self.0.len() {
            f.write_str(", ...")?;
        }

        f.write_str("]")?;
        Ok(())
    }
}

// Note: If required, a `Display` implementation can be added easily for `LimitSlice`.

#[cfg(test)]
mod tests {
    use crate::utils::fmt_limit::LimitSlice;

    #[test]
    fn limit_debug_works() {
        let collections: Vec<_> = (0..5).into_iter().collect();

        // Sanity check.
        assert_eq!(format!("{:?}", collections), "[0, 1, 2, 3, 4]");

        assert_eq!(
            format!("{:?}", LimitSlice::<'_, _, 3>::new(collections.as_slice())),
            "[0, 1, 2, ...]"
        );

        assert_eq!(
            format!("{:?}", LimitSlice::<'_, _, 5>::new(collections.as_slice())),
            "[0, 1, 2, 3, 4]"
        );

        assert_eq!(
            format!("{:?}", LimitSlice::<'_, _, 4>::new(collections.as_slice())),
            "[0, 1, 2, 3, ...]"
        );

        assert_eq!(
            format!("{:?}", LimitSlice::<'_, _, 6>::new(collections.as_slice())),
            "[0, 1, 2, 3, 4]"
        );

        assert_eq!(
            format!("{:?}", LimitSlice::<'_, _, 1>::new(collections.as_slice())),
            "[0, ...]"
        );

        // This does not make a lot of sense at all, but it's there.
        assert_eq!(
            format!("{:?}", LimitSlice::<'_, _, 0>::new(collections.as_slice())),
            "[, ...]"
        );

        // Edge case: Empty slice:
        assert_eq!(format!("{:?}", LimitSlice::<'_, usize, 5>::new(&[])), "[]");
    }
}
