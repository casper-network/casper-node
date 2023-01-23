use std::{
    iter::Sum,
    ops::{Div, Mul},
};

use datasize::DataSize;
use derive_more::{Add, AddAssign, From, Sub, SubAssign, Sum};
use serde::Serialize;

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
    Serialize,
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

    /// Saturating addition. Returns `Weight(u64::MAX)` if overflow would occur.
    pub fn saturating_add(self, rhs: Weight) -> Weight {
        Weight(self.0.saturating_add(rhs.0))
    }

    /// Returns `true` if this weight is zero.
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl<'a> Sum<&'a Weight> for Weight {
    fn sum<I: Iterator<Item = &'a Weight>>(iter: I) -> Self {
        Weight(iter.map(|w| w.0).sum())
    }
}

impl Mul<u64> for Weight {
    type Output = Self;

    #[allow(clippy::integer_arithmetic)] // The caller needs to prevent overflows.
    fn mul(self, rhs: u64) -> Self {
        Weight(self.0 * rhs)
    }
}

impl Div<u64> for Weight {
    type Output = Self;

    #[allow(clippy::integer_arithmetic)] // The caller needs to avoid dividing by zero.
    fn div(self, rhs: u64) -> Self {
        Weight(self.0 / rhs)
    }
}

impl From<Weight> for u128 {
    fn from(Weight(w): Weight) -> u128 {
        u128::from(w)
    }
}
