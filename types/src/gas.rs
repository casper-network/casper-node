//! The `gas` module is used for working with Gas including converting to and from Motes.

use alloc::vec::Vec;
use core::{
    fmt,
    iter::Sum,
    ops::{Add, AddAssign, Div, Mul, Sub},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use num::Zero;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Motes, U512,
};

/// The `Gas` struct represents a `U512` amount of gas.
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Gas(U512);

impl Gas {
    /// Constructs a new `Gas`.
    pub fn new<T: Into<U512>>(value: T) -> Self {
        Gas(value.into())
    }

    /// Constructs a new `Gas` with value `0`.
    pub const fn zero() -> Self {
        Gas(U512::zero())
    }

    /// Returns the inner `U512` value.
    pub fn value(&self) -> U512 {
        self.0
    }

    /// Returns the cost to be charged.
    pub fn cost(&self, is_system: bool) -> Self {
        if is_system {
            return Gas::new(U512::zero());
        }
        *self
    }

    /// Converts the given `Motes` to `Gas` by dividing them by `motes_per_unit_of_gas`, rounding
    /// down.
    ///
    /// Returns `None` if `motes_per_unit_of_gas == 0`.
    pub fn from_motes(motes: Motes, motes_per_unit_of_gas: u64) -> Option<Self> {
        motes
            .value()
            .checked_div(U512::from(motes_per_unit_of_gas))
            .map(Self::new)
    }

    /// Checked integer addition. Computes `self + rhs`, returning `None` if overflow occurred.
    pub fn checked_add(&self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.value()).map(Self::new)
    }

    /// Checked integer subtraction. Computes `self - rhs`, returning `None` if overflow occurred.
    pub fn checked_sub(&self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.value()).map(Self::new)
    }
}

impl ToBytes for Gas {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }
}

impl FromBytes for Gas {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, remainder) = U512::from_bytes(bytes)?;
        Ok((Gas(value), remainder))
    }
}

impl fmt::Display for Gas {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Add for Gas {
    type Output = Gas;

    fn add(self, rhs: Self) -> Self::Output {
        let val = self.value() + rhs.value();
        Gas::new(val)
    }
}

impl Sub for Gas {
    type Output = Gas;

    fn sub(self, rhs: Self) -> Self::Output {
        let val = self.value() - rhs.value();
        Gas::new(val)
    }
}

impl Div for Gas {
    type Output = Gas;

    fn div(self, rhs: Self) -> Self::Output {
        let val = self.value() / rhs.value();
        Gas::new(val)
    }
}

impl Mul for Gas {
    type Output = Gas;

    fn mul(self, rhs: Self) -> Self::Output {
        let val = self.value() * rhs.value();
        Gas::new(val)
    }
}

impl AddAssign for Gas {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0
    }
}

impl Zero for Gas {
    fn zero() -> Self {
        Gas::new(U512::zero())
    }

    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

impl Sum for Gas {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Gas::zero(), Add::add)
    }
}

impl From<u32> for Gas {
    fn from(gas: u32) -> Self {
        let gas_u512: U512 = gas.into();
        Gas::new(gas_u512)
    }
}

impl From<u64> for Gas {
    fn from(gas: u64) -> Self {
        let gas_u512: U512 = gas.into();
        Gas::new(gas_u512)
    }
}

#[cfg(test)]
mod tests {
    use crate::U512;

    use crate::{Gas, Motes};

    #[test]
    fn should_be_able_to_get_instance_of_gas() {
        let initial_value = 1;
        let gas = Gas::new(U512::from(initial_value));
        assert_eq!(
            initial_value,
            gas.value().as_u64(),
            "should have equal value"
        )
    }

    #[test]
    fn should_be_able_to_compare_two_instances_of_gas() {
        let left_gas = Gas::new(U512::from(1));
        let right_gas = Gas::new(U512::from(1));
        assert_eq!(left_gas, right_gas, "should be equal");
        let right_gas = Gas::new(U512::from(2));
        assert_ne!(left_gas, right_gas, "should not be equal")
    }

    #[test]
    fn should_be_able_to_add_two_instances_of_gas() {
        let left_gas = Gas::new(U512::from(1));
        let right_gas = Gas::new(U512::from(1));
        let expected_gas = Gas::new(U512::from(2));
        assert_eq!((left_gas + right_gas), expected_gas, "should be equal")
    }

    #[test]
    fn should_be_able_to_subtract_two_instances_of_gas() {
        let left_gas = Gas::new(U512::from(1));
        let right_gas = Gas::new(U512::from(1));
        let expected_gas = Gas::new(U512::from(0));
        assert_eq!((left_gas - right_gas), expected_gas, "should be equal")
    }

    #[test]
    fn should_be_able_to_multiply_two_instances_of_gas() {
        let left_gas = Gas::new(U512::from(100));
        let right_gas = Gas::new(U512::from(10));
        let expected_gas = Gas::new(U512::from(1000));
        assert_eq!((left_gas * right_gas), expected_gas, "should be equal")
    }

    #[test]
    fn should_be_able_to_divide_two_instances_of_gas() {
        let left_gas = Gas::new(U512::from(1000));
        let right_gas = Gas::new(U512::from(100));
        let expected_gas = Gas::new(U512::from(10));
        assert_eq!((left_gas / right_gas), expected_gas, "should be equal")
    }

    #[test]
    fn should_be_able_to_convert_from_mote() {
        let mote = Motes::new(U512::from(100));
        let gas = Gas::from_motes(mote, 10).expect("should have gas");
        let expected_gas = Gas::new(U512::from(10));
        assert_eq!(gas, expected_gas, "should be equal")
    }

    #[test]
    fn should_be_able_to_default() {
        let gas = Gas::default();
        let expected_gas = Gas::zero();
        assert_eq!(gas, expected_gas, "should be equal")
    }

    #[test]
    fn should_be_able_to_compare_relative_value() {
        let left_gas = Gas::new(U512::from(100));
        let right_gas = Gas::new(U512::from(10));
        assert!(left_gas > right_gas, "should be gt");
        let right_gas = Gas::new(U512::from(100));
        assert!(left_gas >= right_gas, "should be gte");
        assert!(left_gas <= right_gas, "should be lte");
        let left_gas = Gas::new(U512::from(10));
        assert!(left_gas < right_gas, "should be lt");
    }

    #[test]
    fn should_support_checked_div_from_motes() {
        let motes = Motes::new(U512::zero());
        let conv_rate = 0;
        let maybe = Gas::from_motes(motes, conv_rate);
        assert!(maybe.is_none(), "should be none due to divide by zero");
    }
}
