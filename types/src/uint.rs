use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    fmt::{self, Formatter},
    iter::Sum,
    ops::Add,
};

use num_integer::Integer;
use num_traits::{
    AsPrimitive, Bounded, CheckedAdd, CheckedMul, CheckedSub, Num, One, Unsigned, WrappingAdd,
    WrappingSub, Zero,
};
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{
    de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor},
    ser::{Serialize, SerializeStruct, Serializer},
};

use crate::bytesrepr::{self, Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

#[allow(
    clippy::assign_op_pattern,
    clippy::ptr_offset_with_cast,
    clippy::manual_range_contains,
    clippy::range_plus_one,
    clippy::transmute_ptr_to_ptr,
    clippy::reversed_empty_ranges
)]
mod macro_code {
    #[cfg(feature = "datasize")]
    use datasize::DataSize;
    use uint::construct_uint;

    construct_uint! {
        #[cfg_attr(feature = "datasize", derive(DataSize))]
        pub struct U512(8);
    }
    construct_uint! {
        #[cfg_attr(feature = "datasize", derive(DataSize))]
        pub struct U256(4);
    }
    construct_uint! {
        #[cfg_attr(feature = "datasize", derive(DataSize))]
        pub struct U128(2);
    }
}

pub use self::macro_code::{U128, U256, U512};

/// Error type for parsing [`U128`], [`U256`], [`U512`] from a string.
#[derive(Debug)]
#[non_exhaustive]
pub enum UIntParseError {
    /// Contains the parsing error from the `uint` crate, which only supports base-10 parsing.
    FromDecStr(uint::FromDecStrErr),
    /// Parsing was attempted on a string representing the number in some base other than 10.
    ///
    /// Note: a general radix may be supported in the future.
    InvalidRadix,
}

macro_rules! impl_traits_for_uint {
    ($type:ident, $total_bytes:expr, $test_mod:ident) => {
        impl Serialize for $type {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                if serializer.is_human_readable() {
                    return self.to_string().serialize(serializer);
                }

                let mut buffer = [0u8; $total_bytes];
                self.to_little_endian(&mut buffer);
                let non_zero_bytes: Vec<u8> = buffer
                    .iter()
                    .rev()
                    .skip_while(|b| **b == 0)
                    .cloned()
                    .collect();
                let num_bytes = non_zero_bytes.len();

                let mut state = serializer.serialize_struct("bigint", num_bytes + 1)?;
                state.serialize_field("", &(num_bytes as u8))?;

                for byte in non_zero_bytes.into_iter().rev() {
                    state.serialize_field("", &byte)?;
                }
                state.end()
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct BigNumVisitor;

                impl<'de> Visitor<'de> for BigNumVisitor {
                    type Value = $type;

                    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                        formatter.write_str("bignum struct")
                    }

                    fn visit_seq<V: SeqAccess<'de>>(
                        self,
                        mut sequence: V,
                    ) -> Result<$type, V::Error> {
                        let length: u8 = sequence
                            .next_element()?
                            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                        let mut buffer = [0u8; $total_bytes];
                        for index in 0..length as usize {
                            let value = sequence
                                .next_element()?
                                .ok_or_else(|| de::Error::invalid_length(index + 1, &self))?;
                            buffer[index as usize] = value;
                        }
                        let result = $type::from_little_endian(&buffer);
                        Ok(result)
                    }

                    fn visit_map<V: MapAccess<'de>>(self, mut map: V) -> Result<$type, V::Error> {
                        let _length_key: u8 = map
                            .next_key()?
                            .ok_or_else(|| de::Error::missing_field("length"))?;
                        let length: u8 = map
                            .next_value()
                            .map_err(|_| de::Error::invalid_length(0, &self))?;
                        let mut buffer = [0u8; $total_bytes];
                        for index in 0..length {
                            let _byte_key: u8 = map
                                .next_key()?
                                .ok_or_else(|| de::Error::missing_field("byte"))?;
                            let value = map.next_value().map_err(|_| {
                                de::Error::invalid_length(index as usize + 1, &self)
                            })?;
                            buffer[index as usize] = value;
                        }
                        let result = $type::from_little_endian(&buffer);
                        Ok(result)
                    }
                }

                const FIELDS: &'static [&'static str] = &[
                    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14",
                    "15", "16", "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27",
                    "28", "29", "30", "31", "32", "33", "34", "35", "36", "37", "38", "39", "40",
                    "41", "42", "43", "44", "45", "46", "47", "48", "49", "50", "51", "52", "53",
                    "54", "55", "56", "57", "58", "59", "60", "61", "62", "63", "64",
                ];

                if deserializer.is_human_readable() {
                    let decimal_string = String::deserialize(deserializer)?;
                    return Self::from_dec_str(&decimal_string)
                        .map_err(|error| de::Error::custom(format!("{:?}", error)));
                }

                deserializer.deserialize_struct("bigint", FIELDS, BigNumVisitor)
            }
        }

        impl ToBytes for $type {
            fn to_bytes(&self) -> Result<Vec<u8>, Error> {
                let mut buf = [0u8; $total_bytes];
                self.to_little_endian(&mut buf);
                let mut non_zero_bytes: Vec<u8> =
                    buf.iter().rev().skip_while(|b| **b == 0).cloned().collect();
                let num_bytes = non_zero_bytes.len() as u8;
                non_zero_bytes.push(num_bytes);
                non_zero_bytes.reverse();
                Ok(non_zero_bytes)
            }

            fn serialized_length(&self) -> usize {
                let mut buf = [0u8; $total_bytes];
                self.to_little_endian(&mut buf);
                let non_zero_bytes = buf.iter().rev().skip_while(|b| **b == 0).count();
                U8_SERIALIZED_LENGTH + non_zero_bytes
            }
        }

        impl FromBytes for $type {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
                let (num_bytes, rem): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;

                if num_bytes > $total_bytes {
                    Err(Error::Formatting)
                } else {
                    let (value, rem) = bytesrepr::safe_split_at(rem, num_bytes as usize)?;
                    let result = $type::from_little_endian(value);
                    Ok((result, rem))
                }
            }
        }

        // Trait implementations for unifying U* as numeric types
        impl Zero for $type {
            fn zero() -> Self {
                $type::zero()
            }

            fn is_zero(&self) -> bool {
                self.is_zero()
            }
        }

        impl One for $type {
            fn one() -> Self {
                $type::one()
            }
        }

        // Requires Zero and One to be implemented
        impl Num for $type {
            type FromStrRadixErr = UIntParseError;
            fn from_str_radix(str: &str, radix: u32) -> Result<Self, Self::FromStrRadixErr> {
                if radix == 10 {
                    $type::from_dec_str(str).map_err(UIntParseError::FromDecStr)
                } else {
                    // TODO: other radix parsing
                    Err(UIntParseError::InvalidRadix)
                }
            }
        }

        // Requires Num to be implemented
        impl Unsigned for $type {}

        // Additional numeric trait, which also holds for these types
        impl Bounded for $type {
            fn min_value() -> Self {
                $type::zero()
            }

            fn max_value() -> Self {
                $type::MAX
            }
        }

        // Instead of implementing arbitrary methods we can use existing traits from num_trait
        // crate.
        impl WrappingAdd for $type {
            fn wrapping_add(&self, other: &$type) -> $type {
                self.overflowing_add(*other).0
            }
        }

        impl WrappingSub for $type {
            fn wrapping_sub(&self, other: &$type) -> $type {
                self.overflowing_sub(*other).0
            }
        }

        impl CheckedMul for $type {
            fn checked_mul(&self, v: &$type) -> Option<$type> {
                $type::checked_mul(*self, *v)
            }
        }

        impl CheckedSub for $type {
            fn checked_sub(&self, v: &$type) -> Option<$type> {
                $type::checked_sub(*self, *v)
            }
        }

        impl CheckedAdd for $type {
            fn checked_add(&self, v: &$type) -> Option<$type> {
                $type::checked_add(*self, *v)
            }
        }

        impl Integer for $type {
            /// Unsigned integer division. Returns the same result as `div` (`/`).
            #[inline]
            fn div_floor(&self, other: &Self) -> Self {
                *self / *other
            }

            /// Unsigned integer modulo operation. Returns the same result as `rem` (`%`).
            #[inline]
            fn mod_floor(&self, other: &Self) -> Self {
                *self % *other
            }

            /// Calculates the Greatest Common Divisor (GCD) of the number and `other`
            #[inline]
            fn gcd(&self, other: &Self) -> Self {
                let zero = Self::zero();
                // Use Stein's algorithm
                let mut m = *self;
                let mut n = *other;
                if m == zero || n == zero {
                    return m | n;
                }

                // find common factors of 2
                let shift = (m | n).trailing_zeros();

                // divide n and m by 2 until odd
                m >>= m.trailing_zeros();
                n >>= n.trailing_zeros();

                while m != n {
                    if m > n {
                        m -= n;
                        m >>= m.trailing_zeros();
                    } else {
                        n -= m;
                        n >>= n.trailing_zeros();
                    }
                }
                m << shift
            }

            /// Calculates the Lowest Common Multiple (LCM) of the number and `other`.
            #[inline]
            fn lcm(&self, other: &Self) -> Self {
                self.gcd_lcm(other).1
            }

            /// Calculates the Greatest Common Divisor (GCD) and
            /// Lowest Common Multiple (LCM) of the number and `other`.
            #[inline]
            fn gcd_lcm(&self, other: &Self) -> (Self, Self) {
                if self.is_zero() && other.is_zero() {
                    return (Self::zero(), Self::zero());
                }
                let gcd = self.gcd(other);
                let lcm = *self * (*other / gcd);
                (gcd, lcm)
            }

            /// Deprecated, use `is_multiple_of` instead.
            #[inline]
            fn divides(&self, other: &Self) -> bool {
                self.is_multiple_of(other)
            }

            /// Returns `true` if the number is a multiple of `other`.
            #[inline]
            fn is_multiple_of(&self, other: &Self) -> bool {
                *self % *other == $type::zero()
            }

            /// Returns `true` if the number is divisible by `2`.
            #[inline]
            fn is_even(&self) -> bool {
                (self.0[0]) & 1 == 0
            }

            /// Returns `true` if the number is not divisible by `2`.
            #[inline]
            fn is_odd(&self) -> bool {
                !self.is_even()
            }

            /// Simultaneous truncated integer division and modulus.
            #[inline]
            fn div_rem(&self, other: &Self) -> (Self, Self) {
                (*self / *other, *self % *other)
            }
        }

        impl AsPrimitive<$type> for i32 {
            fn as_(self) -> $type {
                if self >= 0 {
                    $type::from(self as u32)
                } else {
                    let abs = 0u32.wrapping_sub(self as u32);
                    $type::zero().wrapping_sub(&$type::from(abs))
                }
            }
        }

        impl AsPrimitive<$type> for i64 {
            fn as_(self) -> $type {
                if self >= 0 {
                    $type::from(self as u64)
                } else {
                    let abs = 0u64.wrapping_sub(self as u64);
                    $type::zero().wrapping_sub(&$type::from(abs))
                }
            }
        }

        impl AsPrimitive<$type> for u8 {
            fn as_(self) -> $type {
                $type::from(self)
            }
        }

        impl AsPrimitive<$type> for u32 {
            fn as_(self) -> $type {
                $type::from(self)
            }
        }

        impl AsPrimitive<$type> for u64 {
            fn as_(self) -> $type {
                $type::from(self)
            }
        }

        impl AsPrimitive<i32> for $type {
            fn as_(self) -> i32 {
                self.0[0] as i32
            }
        }

        impl AsPrimitive<i64> for $type {
            fn as_(self) -> i64 {
                self.0[0] as i64
            }
        }

        impl AsPrimitive<u8> for $type {
            fn as_(self) -> u8 {
                self.0[0] as u8
            }
        }

        impl AsPrimitive<u32> for $type {
            fn as_(self) -> u32 {
                self.0[0] as u32
            }
        }

        impl AsPrimitive<u64> for $type {
            fn as_(self) -> u64 {
                self.0[0]
            }
        }

        impl Sum for $type {
            fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
                iter.fold($type::zero(), Add::add)
            }
        }

        impl Distribution<$type> for Standard {
            fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> $type {
                let mut raw_bytes = [0u8; $total_bytes];
                rng.fill_bytes(raw_bytes.as_mut());
                $type::from(raw_bytes)
            }
        }

        #[cfg(feature = "json-schema")]
        impl schemars::JsonSchema for $type {
            fn schema_name() -> String {
                format!("U{}", $total_bytes * 8)
            }

            fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
                let schema = gen.subschema_for::<String>();
                let mut schema_object = schema.into_object();
                schema_object.metadata().description = Some(format!(
                    "Decimal representation of a {}-bit integer.",
                    $total_bytes * 8
                ));
                schema_object.into()
            }
        }

        #[cfg(test)]
        mod $test_mod {
            use super::*;

            #[test]
            fn test_div_mod_floor() {
                assert_eq!($type::from(10).div_floor(&$type::from(3)), $type::from(3));
                assert_eq!($type::from(10).mod_floor(&$type::from(3)), $type::from(1));
                assert_eq!(
                    $type::from(10).div_mod_floor(&$type::from(3)),
                    ($type::from(3), $type::from(1))
                );
                assert_eq!($type::from(5).div_floor(&$type::from(5)), $type::from(1));
                assert_eq!($type::from(5).mod_floor(&$type::from(5)), $type::from(0));
                assert_eq!(
                    $type::from(5).div_mod_floor(&$type::from(5)),
                    ($type::from(1), $type::from(0))
                );
                assert_eq!($type::from(3).div_floor(&$type::from(7)), $type::from(0));
                assert_eq!($type::from(3).mod_floor(&$type::from(7)), $type::from(3));
                assert_eq!(
                    $type::from(3).div_mod_floor(&$type::from(7)),
                    ($type::from(0), $type::from(3))
                );
            }

            #[test]
            fn test_gcd() {
                assert_eq!($type::from(10).gcd(&$type::from(2)), $type::from(2));
                assert_eq!($type::from(10).gcd(&$type::from(3)), $type::from(1));
                assert_eq!($type::from(0).gcd(&$type::from(3)), $type::from(3));
                assert_eq!($type::from(3).gcd(&$type::from(3)), $type::from(3));
                assert_eq!($type::from(56).gcd(&$type::from(42)), $type::from(14));
                assert_eq!(
                    $type::MAX.gcd(&($type::MAX / $type::from(2))),
                    $type::from(1)
                );
                assert_eq!($type::from(15).gcd(&$type::from(17)), $type::from(1));
            }

            #[test]
            fn test_lcm() {
                assert_eq!($type::from(1).lcm(&$type::from(0)), $type::from(0));
                assert_eq!($type::from(0).lcm(&$type::from(1)), $type::from(0));
                assert_eq!($type::from(1).lcm(&$type::from(1)), $type::from(1));
                assert_eq!($type::from(8).lcm(&$type::from(9)), $type::from(72));
                assert_eq!($type::from(11).lcm(&$type::from(5)), $type::from(55));
                assert_eq!($type::from(15).lcm(&$type::from(17)), $type::from(255));
                assert_eq!($type::from(4).lcm(&$type::from(8)), $type::from(8));
            }

            #[test]
            fn test_is_multiple_of() {
                assert!($type::from(6).is_multiple_of(&$type::from(6)));
                assert!($type::from(6).is_multiple_of(&$type::from(3)));
                assert!($type::from(6).is_multiple_of(&$type::from(1)));
                assert!(!$type::from(3).is_multiple_of(&$type::from(5)))
            }

            #[test]
            fn is_even() {
                assert_eq!($type::from(0).is_even(), true);
                assert_eq!($type::from(1).is_even(), false);
                assert_eq!($type::from(2).is_even(), true);
                assert_eq!($type::from(3).is_even(), false);
                assert_eq!($type::from(4).is_even(), true);
            }

            #[test]
            fn is_odd() {
                assert_eq!($type::from(0).is_odd(), false);
                assert_eq!($type::from(1).is_odd(), true);
                assert_eq!($type::from(2).is_odd(), false);
                assert_eq!($type::from(3).is_odd(), true);
                assert_eq!($type::from(4).is_odd(), false);
            }

            #[test]
            #[should_panic]
            fn overflow_mul_test() {
                let _ = $type::MAX * $type::from(2);
            }

            #[test]
            #[should_panic]
            fn overflow_add_test() {
                let _ = $type::MAX + $type::from(1);
            }

            #[test]
            #[should_panic]
            fn underflow_sub_test() {
                let _ = $type::zero() - $type::from(1);
            }
        }
    };
}

impl_traits_for_uint!(U128, 16, u128_test);
impl_traits_for_uint!(U256, 32, u256_test);
impl_traits_for_uint!(U512, 64, u512_test);

impl AsPrimitive<U128> for U128 {
    fn as_(self) -> U128 {
        self
    }
}

impl AsPrimitive<U256> for U128 {
    fn as_(self) -> U256 {
        let mut result = U256::zero();
        result.0[..2].clone_from_slice(&self.0[..2]);
        result
    }
}

impl AsPrimitive<U512> for U128 {
    fn as_(self) -> U512 {
        let mut result = U512::zero();
        result.0[..2].clone_from_slice(&self.0[..2]);
        result
    }
}

impl AsPrimitive<U128> for U256 {
    fn as_(self) -> U128 {
        let mut result = U128::zero();
        result.0[..2].clone_from_slice(&self.0[..2]);
        result
    }
}

impl AsPrimitive<U256> for U256 {
    fn as_(self) -> U256 {
        self
    }
}

impl AsPrimitive<U512> for U256 {
    fn as_(self) -> U512 {
        let mut result = U512::zero();
        result.0[..4].clone_from_slice(&self.0[..4]);
        result
    }
}

impl AsPrimitive<U128> for U512 {
    fn as_(self) -> U128 {
        let mut result = U128::zero();
        result.0[..2].clone_from_slice(&self.0[..2]);
        result
    }
}

impl AsPrimitive<U256> for U512 {
    fn as_(self) -> U256 {
        let mut result = U256::zero();
        result.0[..4].clone_from_slice(&self.0[..4]);
        result
    }
}

impl AsPrimitive<U512> for U512 {
    fn as_(self) -> U512 {
        self
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use serde::de::DeserializeOwned;

    use super::*;

    fn check_as_i32<T: AsPrimitive<i32>>(expected: i32, input: T) {
        assert_eq!(expected, input.as_());
    }

    fn check_as_i64<T: AsPrimitive<i64>>(expected: i64, input: T) {
        assert_eq!(expected, input.as_());
    }

    fn check_as_u8<T: AsPrimitive<u8>>(expected: u8, input: T) {
        assert_eq!(expected, input.as_());
    }

    fn check_as_u32<T: AsPrimitive<u32>>(expected: u32, input: T) {
        assert_eq!(expected, input.as_());
    }

    fn check_as_u64<T: AsPrimitive<u64>>(expected: u64, input: T) {
        assert_eq!(expected, input.as_());
    }

    fn check_as_u128<T: AsPrimitive<U128>>(expected: U128, input: T) {
        assert_eq!(expected, input.as_());
    }

    fn check_as_u256<T: AsPrimitive<U256>>(expected: U256, input: T) {
        assert_eq!(expected, input.as_());
    }

    fn check_as_u512<T: AsPrimitive<U512>>(expected: U512, input: T) {
        assert_eq!(expected, input.as_());
    }

    #[test]
    fn as_primitive_from_i32() {
        let mut input = 0_i32;
        check_as_i32(0, input);
        check_as_i64(0, input);
        check_as_u8(0, input);
        check_as_u32(0, input);
        check_as_u64(0, input);
        check_as_u128(U128::zero(), input);
        check_as_u256(U256::zero(), input);
        check_as_u512(U512::zero(), input);

        input = i32::max_value() - 1;
        check_as_i32(input, input);
        check_as_i64(i64::from(input), input);
        check_as_u8(input as u8, input);
        check_as_u32(input as u32, input);
        check_as_u64(input as u64, input);
        check_as_u128(U128::from(input), input);
        check_as_u256(U256::from(input), input);
        check_as_u512(U512::from(input), input);

        input = i32::min_value() + 1;
        check_as_i32(input, input);
        check_as_i64(i64::from(input), input);
        check_as_u8(input as u8, input);
        check_as_u32(input as u32, input);
        check_as_u64(input as u64, input);
        // i32::min_value() is -1 - i32::max_value()
        check_as_u128(
            U128::zero().wrapping_sub(&U128::from(i32::max_value())),
            input,
        );
        check_as_u256(
            U256::zero().wrapping_sub(&U256::from(i32::max_value())),
            input,
        );
        check_as_u512(
            U512::zero().wrapping_sub(&U512::from(i32::max_value())),
            input,
        );
    }

    #[test]
    fn as_primitive_from_i64() {
        let mut input = 0_i64;
        check_as_i32(0, input);
        check_as_i64(0, input);
        check_as_u8(0, input);
        check_as_u32(0, input);
        check_as_u64(0, input);
        check_as_u128(U128::zero(), input);
        check_as_u256(U256::zero(), input);
        check_as_u512(U512::zero(), input);

        input = i64::max_value() - 1;
        check_as_i32(input as i32, input);
        check_as_i64(input, input);
        check_as_u8(input as u8, input);
        check_as_u32(input as u32, input);
        check_as_u64(input as u64, input);
        check_as_u128(U128::from(input), input);
        check_as_u256(U256::from(input), input);
        check_as_u512(U512::from(input), input);

        input = i64::min_value() + 1;
        check_as_i32(input as i32, input);
        check_as_i64(input, input);
        check_as_u8(input as u8, input);
        check_as_u32(input as u32, input);
        check_as_u64(input as u64, input);
        // i64::min_value() is (-1 - i64::max_value())
        check_as_u128(
            U128::zero().wrapping_sub(&U128::from(i64::max_value())),
            input,
        );
        check_as_u256(
            U256::zero().wrapping_sub(&U256::from(i64::max_value())),
            input,
        );
        check_as_u512(
            U512::zero().wrapping_sub(&U512::from(i64::max_value())),
            input,
        );
    }

    #[test]
    fn as_primitive_from_u8() {
        let mut input = 0_u8;
        check_as_i32(0, input);
        check_as_i64(0, input);
        check_as_u8(0, input);
        check_as_u32(0, input);
        check_as_u64(0, input);
        check_as_u128(U128::zero(), input);
        check_as_u256(U256::zero(), input);
        check_as_u512(U512::zero(), input);

        input = u8::max_value() - 1;
        check_as_i32(i32::from(input), input);
        check_as_i64(i64::from(input), input);
        check_as_u8(input, input);
        check_as_u32(u32::from(input), input);
        check_as_u64(u64::from(input), input);
        check_as_u128(U128::from(input), input);
        check_as_u256(U256::from(input), input);
        check_as_u512(U512::from(input), input);
    }

    #[test]
    fn as_primitive_from_u32() {
        let mut input = 0_u32;
        check_as_i32(0, input);
        check_as_i64(0, input);
        check_as_u8(0, input);
        check_as_u32(0, input);
        check_as_u64(0, input);
        check_as_u128(U128::zero(), input);
        check_as_u256(U256::zero(), input);
        check_as_u512(U512::zero(), input);

        input = u32::max_value() - 1;
        check_as_i32(input as i32, input);
        check_as_i64(i64::from(input), input);
        check_as_u8(input as u8, input);
        check_as_u32(input, input);
        check_as_u64(u64::from(input), input);
        check_as_u128(U128::from(input), input);
        check_as_u256(U256::from(input), input);
        check_as_u512(U512::from(input), input);
    }

    #[test]
    fn as_primitive_from_u64() {
        let mut input = 0_u64;
        check_as_i32(0, input);
        check_as_i64(0, input);
        check_as_u8(0, input);
        check_as_u32(0, input);
        check_as_u64(0, input);
        check_as_u128(U128::zero(), input);
        check_as_u256(U256::zero(), input);
        check_as_u512(U512::zero(), input);

        input = u64::max_value() - 1;
        check_as_i32(input as i32, input);
        check_as_i64(input as i64, input);
        check_as_u8(input as u8, input);
        check_as_u32(input as u32, input);
        check_as_u64(input, input);
        check_as_u128(U128::from(input), input);
        check_as_u256(U256::from(input), input);
        check_as_u512(U512::from(input), input);
    }

    fn make_little_endian_arrays(little_endian_bytes: &[u8]) -> ([u8; 4], [u8; 8]) {
        let le_32 = {
            let mut le_32 = [0; 4];
            le_32.copy_from_slice(&little_endian_bytes[..4]);
            le_32
        };

        let le_64 = {
            let mut le_64 = [0; 8];
            le_64.copy_from_slice(&little_endian_bytes[..8]);
            le_64
        };

        (le_32, le_64)
    }

    #[test]
    fn as_primitive_from_u128() {
        let mut input = U128::zero();
        check_as_i32(0, input);
        check_as_i64(0, input);
        check_as_u8(0, input);
        check_as_u32(0, input);
        check_as_u64(0, input);
        check_as_u128(U128::zero(), input);
        check_as_u256(U256::zero(), input);
        check_as_u512(U512::zero(), input);

        input = U128::max_value() - 1;

        let mut little_endian_bytes = [0_u8; 64];
        input.to_little_endian(&mut little_endian_bytes[..16]);
        let (le_32, le_64) = make_little_endian_arrays(&little_endian_bytes);

        check_as_i32(i32::from_le_bytes(le_32), input);
        check_as_i64(i64::from_le_bytes(le_64), input);
        check_as_u8(little_endian_bytes[0], input);
        check_as_u32(u32::from_le_bytes(le_32), input);
        check_as_u64(u64::from_le_bytes(le_64), input);
        check_as_u128(U128::from_little_endian(&little_endian_bytes[..16]), input);
        check_as_u256(U256::from_little_endian(&little_endian_bytes[..32]), input);
        check_as_u512(U512::from_little_endian(&little_endian_bytes), input);
    }

    #[test]
    fn as_primitive_from_u256() {
        let mut input = U256::zero();
        check_as_i32(0, input);
        check_as_i64(0, input);
        check_as_u8(0, input);
        check_as_u32(0, input);
        check_as_u64(0, input);
        check_as_u128(U128::zero(), input);
        check_as_u256(U256::zero(), input);
        check_as_u512(U512::zero(), input);

        input = U256::max_value() - 1;

        let mut little_endian_bytes = [0_u8; 64];
        input.to_little_endian(&mut little_endian_bytes[..32]);
        let (le_32, le_64) = make_little_endian_arrays(&little_endian_bytes);

        check_as_i32(i32::from_le_bytes(le_32), input);
        check_as_i64(i64::from_le_bytes(le_64), input);
        check_as_u8(little_endian_bytes[0], input);
        check_as_u32(u32::from_le_bytes(le_32), input);
        check_as_u64(u64::from_le_bytes(le_64), input);
        check_as_u128(U128::from_little_endian(&little_endian_bytes[..16]), input);
        check_as_u256(U256::from_little_endian(&little_endian_bytes[..32]), input);
        check_as_u512(U512::from_little_endian(&little_endian_bytes), input);
    }

    #[test]
    fn as_primitive_from_u512() {
        let mut input = U512::zero();
        check_as_i32(0, input);
        check_as_i64(0, input);
        check_as_u8(0, input);
        check_as_u32(0, input);
        check_as_u64(0, input);
        check_as_u128(U128::zero(), input);
        check_as_u256(U256::zero(), input);
        check_as_u512(U512::zero(), input);

        input = U512::max_value() - 1;

        let mut little_endian_bytes = [0_u8; 64];
        input.to_little_endian(&mut little_endian_bytes);
        let (le_32, le_64) = make_little_endian_arrays(&little_endian_bytes);

        check_as_i32(i32::from_le_bytes(le_32), input);
        check_as_i64(i64::from_le_bytes(le_64), input);
        check_as_u8(little_endian_bytes[0], input);
        check_as_u32(u32::from_le_bytes(le_32), input);
        check_as_u64(u64::from_le_bytes(le_64), input);
        check_as_u128(U128::from_little_endian(&little_endian_bytes[..16]), input);
        check_as_u256(U256::from_little_endian(&little_endian_bytes[..32]), input);
        check_as_u512(U512::from_little_endian(&little_endian_bytes), input);
    }

    #[test]
    fn wrapping_test_u512() {
        let max = U512::max_value();
        let value = max.wrapping_add(&1.into());
        assert_eq!(value, 0.into());

        let min = U512::min_value();
        let value = min.wrapping_sub(&1.into());
        assert_eq!(value, U512::max_value());
    }

    #[test]
    fn wrapping_test_u256() {
        let max = U256::max_value();
        let value = max.wrapping_add(&1.into());
        assert_eq!(value, 0.into());

        let min = U256::min_value();
        let value = min.wrapping_sub(&1.into());
        assert_eq!(value, U256::max_value());
    }

    #[test]
    fn wrapping_test_u128() {
        let max = U128::max_value();
        let value = max.wrapping_add(&1.into());
        assert_eq!(value, 0.into());

        let min = U128::min_value();
        let value = min.wrapping_sub(&1.into());
        assert_eq!(value, U128::max_value());
    }

    fn serde_roundtrip<T: Serialize + DeserializeOwned + Eq + Debug>(value: T) {
        {
            let serialized = bincode::serialize(&value).unwrap();
            let deserialized = bincode::deserialize(serialized.as_slice()).unwrap();
            assert_eq!(value, deserialized);
        }
        {
            let serialized = serde_json::to_string_pretty(&value).unwrap();
            let deserialized = serde_json::from_str(&serialized).unwrap();
            assert_eq!(value, deserialized);
        }
    }

    #[test]
    fn serde_roundtrip_u512() {
        serde_roundtrip(U512::min_value());
        serde_roundtrip(U512::from(1));
        serde_roundtrip(U512::from(u64::max_value()));
        serde_roundtrip(U512::max_value());
    }

    #[test]
    fn serde_roundtrip_u256() {
        serde_roundtrip(U256::min_value());
        serde_roundtrip(U256::from(1));
        serde_roundtrip(U256::from(u64::max_value()));
        serde_roundtrip(U256::max_value());
    }

    #[test]
    fn serde_roundtrip_u128() {
        serde_roundtrip(U128::min_value());
        serde_roundtrip(U128::from(1));
        serde_roundtrip(U128::from(u64::max_value()));
        serde_roundtrip(U128::max_value());
    }
}
