// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, Error, FromBytes, ToBytes},
    U512,
};

const DAY_MILLIS: usize = 24 * 60 * 60 * 1000;
const DAYS_IN_WEEK: usize = 7;
const WEEK_MILLIS: usize = DAYS_IN_WEEK * DAY_MILLIS;

/// Length of total vesting schedule in days.
const VESTING_SCHEDULE_LENGTH_DAYS: usize = 91;
/// Length of total vesting schedule expressed in days.
pub const VESTING_SCHEDULE_LENGTH_MILLIS: u64 =
    VESTING_SCHEDULE_LENGTH_DAYS as u64 * DAY_MILLIS as u64;
/// 91 days / 7 days in a week = 13 weeks
const LOCKED_AMOUNTS_MAX_LENGTH: usize = (VESTING_SCHEDULE_LENGTH_DAYS / DAYS_IN_WEEK) + 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VestingSchedule {
    initial_release_timestamp_millis: u64,
    locked_amounts: Option<[U512; LOCKED_AMOUNTS_MAX_LENGTH]>,
}

fn vesting_schedule_period_to_weeks(vesting_schedule_period_millis: u64) -> usize {
    debug_assert_ne!(DAY_MILLIS, 0);
    debug_assert_ne!(DAYS_IN_WEEK, 0);
    vesting_schedule_period_millis as usize / DAY_MILLIS / DAYS_IN_WEEK
}

impl VestingSchedule {
    pub fn new(initial_release_timestamp_millis: u64) -> Self {
        let locked_amounts = None;
        VestingSchedule {
            initial_release_timestamp_millis,
            locked_amounts,
        }
    }

    /// Initializes vesting schedule with a configured amount of weekly releases.
    ///
    /// Returns `false` if already initialized.
    ///
    /// # Panics
    ///
    /// Panics if `vesting_schedule_period_millis` represents more than 13 weeks.
    pub fn initialize_with_schedule(
        &mut self,
        staked_amount: U512,
        vesting_schedule_period_millis: u64,
    ) -> bool {
        if self.locked_amounts.is_some() {
            return false;
        }

        let locked_amounts_length =
            vesting_schedule_period_to_weeks(vesting_schedule_period_millis);

        assert!(
            locked_amounts_length < LOCKED_AMOUNTS_MAX_LENGTH,
            "vesting schedule period must be less than {} weeks",
            LOCKED_AMOUNTS_MAX_LENGTH,
        );

        if locked_amounts_length == 0 || vesting_schedule_period_millis == 0 {
            // Zero weeks means instant unlock of staked amount.
            self.locked_amounts = Some(Default::default());
            return true;
        }

        let release_period: U512 = U512::from(locked_amounts_length + 1);
        let weekly_release = staked_amount / release_period;

        let mut locked_amounts = [U512::zero(); LOCKED_AMOUNTS_MAX_LENGTH];
        let mut remaining_locked = staked_amount;

        for locked_amount in locked_amounts.iter_mut().take(locked_amounts_length) {
            remaining_locked -= weekly_release;
            *locked_amount = remaining_locked;
        }

        assert_eq!(
            locked_amounts.get(locked_amounts_length),
            Some(&U512::zero()),
            "first element after the schedule should be zero"
        );

        self.locked_amounts = Some(locked_amounts);
        true
    }

    /// Initializes weekly release for a fixed amount of 14 weeks period.
    ///
    /// Returns `false` if already initialized.
    pub fn initialize(&mut self, staked_amount: U512) -> bool {
        self.initialize_with_schedule(staked_amount, VESTING_SCHEDULE_LENGTH_MILLIS)
    }

    pub fn initial_release_timestamp_millis(&self) -> u64 {
        self.initial_release_timestamp_millis
    }

    pub fn locked_amounts(&self) -> Option<&[U512]> {
        let locked_amounts = self.locked_amounts.as_ref()?;
        Some(locked_amounts.as_slice())
    }

    pub fn locked_amount(&self, timestamp_millis: u64) -> Option<U512> {
        let locked_amounts = self.locked_amounts()?;

        let index = {
            let index_timestamp =
                timestamp_millis.checked_sub(self.initial_release_timestamp_millis)?;
            (index_timestamp as usize).checked_div(WEEK_MILLIS)?
        };

        let locked_amount = locked_amounts.get(index).cloned().unwrap_or_default();

        Some(locked_amount)
    }

    /// Checks if this vesting schedule is still under the vesting
    pub(crate) fn is_vesting(
        &self,
        timestamp_millis: u64,
        vesting_schedule_period_millis: u64,
    ) -> bool {
        let vested_period = match self.locked_amounts() {
            Some(locked_amounts) => {
                let vesting_weeks = locked_amounts
                    .iter()
                    .position(|amount| amount.is_zero())
                    .expect("vesting schedule should always have zero at the end"); // SAFETY: at least one zero is guaranteed by `initialize_with_schedule` method

                let vesting_weeks_millis =
                    (vesting_weeks as u64).saturating_mul(WEEK_MILLIS as u64);

                self.initial_release_timestamp_millis()
                    .saturating_add(vesting_weeks_millis)
            }
            None => {
                // Uninitialized yet but we know this will be the configured period of time.
                self.initial_release_timestamp_millis()
                    .saturating_add(vesting_schedule_period_millis)
            }
        };

        timestamp_millis < vested_period
    }
}

impl ToBytes for [U512; LOCKED_AMOUNTS_MAX_LENGTH] {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.iter().map(ToBytes::serialized_length).sum::<usize>()
    }

    #[inline]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        for amount in self {
            amount.write_bytes(writer)?;
        }
        Ok(())
    }
}

impl FromBytes for [U512; LOCKED_AMOUNTS_MAX_LENGTH] {
    fn from_bytes(mut bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let mut result = [U512::zero(); LOCKED_AMOUNTS_MAX_LENGTH];
        for value in &mut result {
            let (amount, rem) = FromBytes::from_bytes(bytes)?;
            *value = amount;
            bytes = rem;
        }
        Ok((result, bytes))
    }
}

impl ToBytes for VestingSchedule {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.append(&mut self.initial_release_timestamp_millis.to_bytes()?);
        result.append(&mut self.locked_amounts.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.initial_release_timestamp_millis.serialized_length()
            + self.locked_amounts.serialized_length()
    }
}

impl FromBytes for VestingSchedule {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (initial_release_timestamp_millis, bytes) = FromBytes::from_bytes(bytes)?;
        let (locked_amounts, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            VestingSchedule {
                initial_release_timestamp_millis,
                locked_amounts,
            },
            bytes,
        ))
    }
}

/// Generators for [`VestingSchedule`]
#[cfg(test)]
mod gens {
    use proptest::{
        array, option,
        prelude::{Arbitrary, Strategy},
    };

    use super::VestingSchedule;
    use crate::gens::u512_arb;

    pub fn vesting_schedule_arb() -> impl Strategy<Value = VestingSchedule> {
        (<u64>::arbitrary(), option::of(array::uniform14(u512_arb()))).prop_map(
            |(initial_release_timestamp_millis, locked_amounts)| VestingSchedule {
                initial_release_timestamp_millis,
                locked_amounts,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use proptest::{prop_assert, proptest};

    use crate::{
        bytesrepr,
        gens::u512_arb,
        system::auction::bid::{
            vesting::{gens::vesting_schedule_arb, vesting_schedule_period_to_weeks, WEEK_MILLIS},
            VestingSchedule,
        },
        U512,
    };

    use super::*;

    /// Default lock-in period of 90 days
    const DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS: u64 = 90 * DAY_MILLIS as u64;
    const RELEASE_TIMESTAMP: u64 = DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;
    const STAKE: u64 = 140;

    const DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS: u64 = 91 * DAY_MILLIS as u64;
    const LOCKED_AMOUNTS_LENGTH: usize =
        (DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS as usize / WEEK_MILLIS) + 1;

    #[test]
    #[should_panic = "vesting schedule period must be less than"]
    fn test_vesting_schedule_exceeding_the_maximum_should_not_panic() {
        let future_date = 98 * DAY_MILLIS as u64;
        let mut vesting_schedule = VestingSchedule::new(RELEASE_TIMESTAMP);
        vesting_schedule.initialize_with_schedule(U512::from(STAKE), future_date);

        assert_eq!(vesting_schedule.locked_amount(0), None);
        assert_eq!(vesting_schedule.locked_amount(RELEASE_TIMESTAMP - 1), None);
    }

    #[test]
    fn test_locked_amount_check_should_not_panic() {
        let mut vesting_schedule = VestingSchedule::new(RELEASE_TIMESTAMP);
        vesting_schedule.initialize(U512::from(STAKE));

        assert_eq!(vesting_schedule.locked_amount(0), None);
        assert_eq!(vesting_schedule.locked_amount(RELEASE_TIMESTAMP - 1), None);
    }

    #[test]
    fn test_locked_with_zero_length_schedule_should_not_panic() {
        let mut vesting_schedule = VestingSchedule::new(RELEASE_TIMESTAMP);
        vesting_schedule.initialize_with_schedule(U512::from(STAKE), 0);

        assert_eq!(vesting_schedule.locked_amount(0), None);
        assert_eq!(vesting_schedule.locked_amount(RELEASE_TIMESTAMP - 1), None);
    }

    #[test]
    fn test_locked_amount() {
        let mut vesting_schedule = VestingSchedule::new(RELEASE_TIMESTAMP);
        vesting_schedule.initialize(U512::from(STAKE));

        let mut timestamp = RELEASE_TIMESTAMP;

        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(130))
        );

        timestamp = RELEASE_TIMESTAMP + WEEK_MILLIS as u64 - 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(130))
        );

        timestamp = RELEASE_TIMESTAMP + WEEK_MILLIS as u64;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(120))
        );

        timestamp = RELEASE_TIMESTAMP + WEEK_MILLIS as u64 + 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(120))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 2) - 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(120))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 2);
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(110))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 2) + 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(110))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 3) - 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(110))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 3);
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(100))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 3) + 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(100))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 12) - 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(20))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 12);
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(10))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 12) + 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(10))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 13) - 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(10))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 13);
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(0))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 13) + 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(0))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 14) - 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(0))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 14);
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(0))
        );

        timestamp = RELEASE_TIMESTAMP + (WEEK_MILLIS as u64 * 14) + 1;
        assert_eq!(
            vesting_schedule.locked_amount(timestamp),
            Some(U512::from(0))
        );
    }

    fn vested_amounts_match_initial_stake(
        initial_stake: U512,
        release_timestamp: u64,
        vesting_schedule_length: u64,
    ) -> bool {
        let mut vesting_schedule = VestingSchedule::new(release_timestamp);
        vesting_schedule.initialize_with_schedule(initial_stake, vesting_schedule_length);

        let mut total_vested_amounts = U512::zero();

        for i in 0..LOCKED_AMOUNTS_LENGTH {
            let timestamp = release_timestamp + (WEEK_MILLIS * i) as u64;
            if let Some(locked_amount) = vesting_schedule.locked_amount(timestamp) {
                let current_vested_amount = initial_stake - locked_amount - total_vested_amounts;
                total_vested_amounts += current_vested_amount
            }
        }

        total_vested_amounts == initial_stake
    }

    #[test]
    fn vested_amounts_conserve_stake() {
        let stake = U512::from(1000);
        assert!(vested_amounts_match_initial_stake(
            stake,
            DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
            DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS,
        ))
    }

    #[test]
    fn is_vesting_with_default_schedule() {
        let initial_stake = U512::from(1000u64);
        let release_timestamp = DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;
        let mut vesting_schedule = VestingSchedule::new(release_timestamp);

        let is_vesting_before: Vec<bool> = (0..LOCKED_AMOUNTS_LENGTH + 1)
            .map(|i| {
                vesting_schedule.is_vesting(
                    release_timestamp + (WEEK_MILLIS * i) as u64,
                    DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS,
                )
            })
            .collect();

        assert_eq!(
            is_vesting_before,
            vec![
                true, true, true, true, true, true, true, true, true, true, true, true, true,
                false, // week after is always set to zero
                false
            ]
        );
        vesting_schedule.initialize(initial_stake);

        let is_vesting_after: Vec<bool> = (0..LOCKED_AMOUNTS_LENGTH + 1)
            .map(|i| {
                vesting_schedule.is_vesting(
                    release_timestamp + (WEEK_MILLIS * i) as u64,
                    DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS,
                )
            })
            .collect();

        assert_eq!(
            is_vesting_after,
            vec![
                true, true, true, true, true, true, true, true, true, true, true, true, true,
                false, // week after is always set to zero
                false,
            ]
        );
    }

    #[test]
    fn should_calculate_vesting_schedule_period_to_weeks() {
        let thirteen_weeks_millis = 13 * 7 * DAY_MILLIS as u64;
        assert_eq!(vesting_schedule_period_to_weeks(thirteen_weeks_millis), 13,);

        assert_eq!(vesting_schedule_period_to_weeks(0), 0);
        assert_eq!(
            vesting_schedule_period_to_weeks(u64::MAX),
            30_500_568_904usize
        );
    }

    proptest! {
        #[test]
        fn prop_total_vested_amounts_conserve_stake(stake in u512_arb()) {
            prop_assert!(vested_amounts_match_initial_stake(
                stake,
                DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
                DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS,
            ))
        }

        #[test]
        fn prop_serialization_roundtrip(vesting_schedule in vesting_schedule_arb()) {
            bytesrepr::test_serialization_roundtrip(&vesting_schedule)
        }
    }
}
