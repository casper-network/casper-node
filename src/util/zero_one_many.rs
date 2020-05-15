use std::{iter, vec};

/// A type that contains zero, one or multiple instances of a value.
///
/// Optimization for when you expect to have a lot of `Vec`s that would have
/// zero or one element. Creating a `One` instance does not cause an allocation.
#[derive(Debug)]
pub enum ZeroOneMany<T> {
    /// Zero elements.
    Zero,

    /// One element.
    One(T),

    /// More than one element.
    Many(Vec<T>),
}

impl<T> ZeroOneMany<T> {
    /// Combine values.
    ///
    /// Does not preserve order.
    pub fn combine_unordered(self, other: Self) -> Self {
        match (self, other) {
            (ZeroOneMany::Zero, r) => r,
            (l, ZeroOneMany::Zero) => l,
            (ZeroOneMany::One(val_l), ZeroOneMany::One(val_r)) => {
                ZeroOneMany::Many(vec![val_l, val_r])
            }
            (ZeroOneMany::One(val), ZeroOneMany::Many(mut vals))
            | (ZeroOneMany::Many(mut vals), ZeroOneMany::One(val)) => {
                vals.push(val);

                ZeroOneMany::Many(vals)
            }
            (ZeroOneMany::Many(mut vals_l), ZeroOneMany::Many(vals_r)) => {
                vals_l.extend(vals_r.into_iter());
                ZeroOneMany::Many(vals_l)
            }
        }
    }

    /// Map all value(s).
    #[inline]
    pub fn map<F, U>(self, f: F) -> ZeroOneMany<U>
    where
        F: Fn(T) -> U,
    {
        match self {
            ZeroOneMany::Zero => ZeroOneMany::Zero,
            ZeroOneMany::One(value) => ZeroOneMany::One(f(value)),
            ZeroOneMany::Many(values) => ZeroOneMany::Many(values.into_iter().map(f).collect()),
        }
    }
}

impl<T> IntoIterator for ZeroOneMany<T> {
    type Item = T;
    type IntoIter = either::Either<
        iter::Empty<Self::Item>,
        either::Either<iter::Once<Self::Item>, vec::IntoIter<Self::Item>>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            ZeroOneMany::Zero => either::Left(iter::empty()),
            ZeroOneMany::One(val) => either::Right(either::Left(iter::once(val))),
            ZeroOneMany::Many(vals) => either::Right(either::Right(vals.into_iter())),
        }
    }
}
