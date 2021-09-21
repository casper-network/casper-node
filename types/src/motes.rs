//! The `motes` module is used for working with Motes.

use alloc::vec::Vec;
use core::{
    fmt,
    iter::Sum,
    ops::{Add, Div, Mul, Sub},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use num::Zero;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Gas, U512,
};

/// A struct representing a number of `Motes`.
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct Motes(U512);

impl Motes {
    /// Constructs a new `Motes`.
    pub fn new(value: U512) -> Motes {
        Motes(value)
    }

    /// Checked integer addition. Computes `self + rhs`, returning `None` if overflow occurred.
    pub fn checked_add(&self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.value()).map(Self::new)
    }

    /// Checked integer subtraction. Computes `self - rhs`, returning `None` if underflow occurred.
    pub fn checked_sub(&self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.value()).map(Self::new)
    }

    /// Returns the inner `U512` value.
    pub fn value(&self) -> U512 {
        self.0
    }

    /// Converts the given `gas` to `Motes` by multiplying them by `conv_rate`.
    ///
    /// Returns `None` if an arithmetic overflow occurred.
    pub fn from_gas(gas: Gas, conv_rate: u64) -> Option<Self> {
        gas.value()
            .checked_mul(U512::from(conv_rate))
            .map(Self::new)
    }
}

impl fmt::Display for Motes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Add for Motes {
    type Output = Motes;

    fn add(self, rhs: Self) -> Self::Output {
        let val = self.value() + rhs.value();
        Motes::new(val)
    }
}

impl Sub for Motes {
    type Output = Motes;

    fn sub(self, rhs: Self) -> Self::Output {
        let val = self.value() - rhs.value();
        Motes::new(val)
    }
}

impl Div for Motes {
    type Output = Motes;

    fn div(self, rhs: Self) -> Self::Output {
        let val = self.value() / rhs.value();
        Motes::new(val)
    }
}

impl Mul for Motes {
    type Output = Motes;

    fn mul(self, rhs: Self) -> Self::Output {
        let val = self.value() * rhs.value();
        Motes::new(val)
    }
}

impl Zero for Motes {
    fn zero() -> Self {
        Motes::new(U512::zero())
    }

    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

impl Sum for Motes {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Motes::zero(), Add::add)
    }
}

impl ToBytes for Motes {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for Motes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, remainder) = FromBytes::from_bytes(bytes)?;
        Ok((Motes::new(value), remainder))
    }
}

#[cfg(test)]
mod tests {
    use crate::U512;

    use crate::{Gas, Motes};

    #[test]
    fn should_be_able_to_get_instance_of_motes() {
        let initial_value = 1;
        let motes = Motes::new(U512::from(initial_value));
        assert_eq!(
            initial_value,
            motes.value().as_u64(),
            "should have equal value"
        )
    }

    #[test]
    fn should_be_able_to_compare_two_instances_of_motes() {
        let left_motes = Motes::new(U512::from(1));
        let right_motes = Motes::new(U512::from(1));
        assert_eq!(left_motes, right_motes, "should be equal");
        let right_motes = Motes::new(U512::from(2));
        assert_ne!(left_motes, right_motes, "should not be equal")
    }

    #[test]
    fn should_be_able_to_add_two_instances_of_motes() {
        let left_motes = Motes::new(U512::from(1));
        let right_motes = Motes::new(U512::from(1));
        let expected_motes = Motes::new(U512::from(2));
        assert_eq!(
            (left_motes + right_motes),
            expected_motes,
            "should be equal"
        )
    }

    #[test]
    fn should_be_able_to_subtract_two_instances_of_motes() {
        let left_motes = Motes::new(U512::from(1));
        let right_motes = Motes::new(U512::from(1));
        let expected_motes = Motes::new(U512::from(0));
        assert_eq!(
            (left_motes - right_motes),
            expected_motes,
            "should be equal"
        )
    }

    #[test]
    fn should_be_able_to_multiply_two_instances_of_motes() {
        let left_motes = Motes::new(U512::from(100));
        let right_motes = Motes::new(U512::from(10));
        let expected_motes = Motes::new(U512::from(1000));
        assert_eq!(
            (left_motes * right_motes),
            expected_motes,
            "should be equal"
        )
    }

    #[test]
    fn should_be_able_to_divide_two_instances_of_motes() {
        let left_motes = Motes::new(U512::from(1000));
        let right_motes = Motes::new(U512::from(100));
        let expected_motes = Motes::new(U512::from(10));
        assert_eq!(
            (left_motes / right_motes),
            expected_motes,
            "should be equal"
        )
    }

    #[test]
    fn should_be_able_to_convert_from_motes() {
        let gas = Gas::new(U512::from(100));
        let motes = Motes::from_gas(gas, 10).expect("should have value");
        let expected_motes = Motes::new(U512::from(1000));
        assert_eq!(motes, expected_motes, "should be equal")
    }

    #[test]
    fn should_be_able_to_default() {
        let motes = Motes::default();
        let expected_motes = Motes::new(U512::from(0));
        assert_eq!(motes, expected_motes, "should be equal")
    }

    #[test]
    fn should_be_able_to_compare_relative_value() {
        let left_motes = Motes::new(U512::from(100));
        let right_motes = Motes::new(U512::from(10));
        assert!(left_motes > right_motes, "should be gt");
        let right_motes = Motes::new(U512::from(100));
        assert!(left_motes >= right_motes, "should be gte");
        assert!(left_motes <= right_motes, "should be lte");
        let left_motes = Motes::new(U512::from(10));
        assert!(left_motes < right_motes, "should be lt");
    }

    #[test]
    fn should_default() {
        let left_motes = Motes::new(U512::from(0));
        let right_motes = Motes::default();
        assert_eq!(left_motes, right_motes, "should be equal");
        let u512 = U512::zero();
        assert_eq!(left_motes.value(), u512, "should be equal");
    }

    #[test]
    fn should_support_checked_mul_from_gas() {
        let gas = Gas::new(U512::MAX);
        let conv_rate = 10;
        let maybe = Motes::from_gas(gas, conv_rate);
        assert!(maybe.is_none(), "should be none due to overflow");
    }
}
