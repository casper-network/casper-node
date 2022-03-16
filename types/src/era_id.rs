// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::vec::Vec;
use core::{
    fmt::{self, Debug, Display, Formatter},
    num::ParseIntError,
    ops::{Add, AddAssign, Sub},
    str::FromStr,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped,
};

/// Era ID newtype.
#[derive(
    Debug, Default, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EraId(u64);

impl EraId {
    /// Maximum possible value an [`EraId`] can hold.
    pub const MAX: EraId = EraId(u64::max_value());

    /// Creates new [`EraId`] instance.
    pub const fn new(value: u64) -> EraId {
        EraId(value)
    }

    /// Returns an iterator over era IDs of `num_eras` future eras starting from current.
    pub fn iter(&self, num_eras: u64) -> impl Iterator<Item = EraId> {
        let current_era_id = self.0;
        (current_era_id..current_era_id + num_eras).map(EraId)
    }

    /// Returns an iterator over era IDs of `num_eras` future eras starting from current, plus the
    /// provided one.
    pub fn iter_inclusive(&self, num_eras: u64) -> impl Iterator<Item = EraId> {
        let current_era_id = self.0;
        (current_era_id..=current_era_id + num_eras).map(EraId)
    }

    /// Returns a successor to current era.
    ///
    /// For `u64::MAX`, this returns `u64::MAX` again: We want to make sure this doesn't panic, and
    /// that era number will never be reached in practice.
    #[must_use]
    pub fn successor(self) -> EraId {
        EraId::from(self.0.saturating_add(1))
    }

    /// Returns the current era plus `x`, or `None` if that would overflow
    pub fn checked_add(&self, x: u64) -> Option<EraId> {
        self.0.checked_add(x).map(EraId)
    }

    /// Returns the current era minus `x`, or `None` if that would be less than `0`.
    pub fn checked_sub(&self, x: u64) -> Option<EraId> {
        self.0.checked_sub(x).map(EraId)
    }

    /// Returns the current era minus `x`, or `0` if that would be less than `0`.
    #[must_use]
    pub fn saturating_sub(&self, x: u64) -> EraId {
        EraId::from(self.0.saturating_sub(x))
    }

    /// Returns the current era plus `x`, or [`EraId::MAX`] if overflow would occur.
    #[must_use]
    pub fn saturating_add(self, rhs: u64) -> EraId {
        EraId(self.0.saturating_add(rhs))
    }

    /// Returns the current era times `x`, or [`EraId::MAX`] if overflow would occur.
    #[must_use]
    pub fn saturating_mul(&self, x: u64) -> EraId {
        EraId::from(self.0.saturating_mul(x))
    }

    /// Returns whether this is era 0.
    pub fn is_genesis(&self) -> bool {
        self.0 == 0
    }

    /// Returns little endian bytes.
    pub fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    /// Returns a raw value held by this [`EraId`] instance.
    ///
    /// You should prefer [`From`] trait implementations over this method where possible.
    pub fn value(self) -> u64 {
        self.0
    }
}

impl FromStr for EraId {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u64::from_str(s).map(EraId)
    }
}

impl Add<u64> for EraId {
    type Output = EraId;

    #[allow(clippy::integer_arithmetic)] // The caller must make sure this doesn't overflow.
    fn add(self, x: u64) -> EraId {
        EraId::from(self.0 + x)
    }
}

impl AddAssign<u64> for EraId {
    fn add_assign(&mut self, x: u64) {
        self.0 += x;
    }
}

impl Sub<u64> for EraId {
    type Output = EraId;

    #[allow(clippy::integer_arithmetic)] // The caller must make sure this doesn't overflow.
    fn sub(self, x: u64) -> EraId {
        EraId::from(self.0 - x)
    }
}

impl Display for EraId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "era {}", self.0)
    }
}

impl From<EraId> for u64 {
    fn from(era_id: EraId) -> Self {
        era_id.value()
    }
}

impl From<u64> for EraId {
    fn from(era_id: u64) -> Self {
        EraId(era_id)
    }
}

impl ToBytes for EraId {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    #[inline(always)]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for EraId {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (id_value, remainder) = u64::from_bytes(bytes)?;
        let era_id = EraId::from(id_value);
        Ok((era_id, remainder))
    }
}

impl CLTyped for EraId {
    fn cl_type() -> CLType {
        CLType::U64
    }
}

impl Distribution<EraId> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EraId {
        EraId(rng.gen_range(0..1_000_000))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::gens::era_id_arb;

    #[test]
    fn should_calculate_correct_inclusive_future_eras() {
        let auction_delay = 3;

        let current_era = EraId::from(42);

        let window: Vec<EraId> = current_era.iter_inclusive(auction_delay).collect();
        assert_eq!(window.len(), auction_delay as usize + 1);
        assert_eq!(window.get(0), Some(&current_era));
        assert_eq!(
            window.iter().rev().next(),
            Some(&(current_era + auction_delay))
        );
    }

    #[test]
    fn should_have_valid_genesis_era_id() {
        let expected_initial_era_id = EraId::from(0);
        assert!(expected_initial_era_id.is_genesis());
        assert!(!expected_initial_era_id.successor().is_genesis())
    }

    proptest! {
        #[test]
        fn bytesrepr_roundtrip(era_id in era_id_arb()) {
            bytesrepr::test_serialization_roundtrip(&era_id);
        }
    }
}
