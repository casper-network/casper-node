use std::{
    iter::Sum,
    ops::{Div, Mul},
};

use datasize::DataSize;
use derive_more::{Add, AddAssign, From, Sub, SubAssign, Sum};

/// A vote weight.
#[derive(
    Copy,
    Clone,
    DataSize,
    Default,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Add,
    Sub,
    AddAssign,
    SubAssign,
    Sum,
    From,
)]
pub(crate) struct Weight(pub(crate) u64);

impl Weight {
    /// Checked addition. Returns `None` if overflow occurred.
    pub fn checked_add(self, rhs: Weight) -> Option<Weight> {
        Some(Weight(self.0.checked_add(rhs.0)?))
    }
}

impl<'a> Sum<&'a Weight> for Weight {
    fn sum<I: Iterator<Item = &'a Weight>>(iter: I) -> Self {
        let mut sum = 0u64;
        iter.for_each(|w| sum += w.0);
        Weight(sum)
    }
}

impl Mul<u64> for Weight {
    type Output = Self;

    fn mul(self, rhs: u64) -> Self {
        Weight(self.0 * rhs)
    }
}

impl Div<u64> for Weight {
    type Output = Self;

    fn div(self, rhs: u64) -> Self {
        Weight(self.0 / rhs)
    }
}

impl From<Weight> for u128 {
    fn from(Weight(w): Weight) -> u128 {
        u128::from(w)
    }
}
