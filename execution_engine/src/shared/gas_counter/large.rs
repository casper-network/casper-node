use std::cmp;

use casper_types::U512;

use crate::shared::gas::Gas;

use super::error::GasLimitError;

pub(crate) const U64_MAX_AS_U512: U512 = U512([u64::MAX, 0, 0, 0, 0, 0, 0, 0]);

#[derive(Debug, Copy, Clone)]
pub struct LargeGasCounter {
    total_limit: U512,
    limit_for_buffer: u64,
    used_buffer: u64,
    rest_of_used: U512,
}

impl LargeGasCounter {
    pub(crate) fn new(limit: Gas, initial_count: Gas) -> Self {
        let mut counter = LargeGasCounter {
            total_limit: limit.value(),
            limit_for_buffer: 0,
            used_buffer: 0,
            rest_of_used: initial_count.value(),
        };
        counter.flush_buffer();
        counter
    }

    pub(crate) fn add(&mut self, additional_gas: u64) -> Result<(), GasLimitError> {
        if self.limit_for_buffer - self.used_buffer < additional_gas {
            // We'd overflow, so flush buffer if required and try again, or fail
            if self.used_buffer != 0 {
                self.flush_buffer();
                self.add(additional_gas)
            } else {
                self.used_buffer = 0;
                self.rest_of_used = self.total_limit;
                Err(GasLimitError {})
            }
        } else {
            // Can't overflow as we already checked this sum is <= `self.limit_for_buffer`
            self.used_buffer += additional_gas;
            Ok(())
        }
    }

    pub(crate) fn add_large(&mut self, mut additional_gas: U512) -> Result<(), GasLimitError> {
        while additional_gas >= U64_MAX_AS_U512 {
            self.add(u64::MAX)?;
            additional_gas -= U64_MAX_AS_U512;
        }
        // Safe as we were draining `additional_gas` until it's smaller than `u64::MAX`
        self.add(additional_gas.as_u64())?;
        Ok(())
    }

    fn flush_buffer(&mut self) {
        self.rest_of_used += U512::from(self.used_buffer);
        self.used_buffer = 0;
        self.limit_for_buffer =
            cmp::min(U64_MAX_AS_U512, self.total_limit - self.rest_of_used).as_u64();
    }

    pub(crate) fn used(&self) -> U512 {
        U512::from(self.used_buffer) + self.rest_of_used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_large_counter() {
        let mut counter = LargeGasCounter::new(
            Gas::new(U64_MAX_AS_U512 * 3) + Gas::new(U512::from(u32::max_value())),
            Gas::default(),
        );
        counter.add(u64::max_value()).unwrap();
        assert_eq!(counter.used(), U64_MAX_AS_U512);
        counter.add(u64::max_value()).unwrap();
        assert_eq!(counter.used(), U64_MAX_AS_U512 * 2);
        counter.add(u64::max_value()).unwrap();
        assert_eq!(counter.used(), U64_MAX_AS_U512 * 3);
        counter.add(u32::max_value().into()).unwrap();
        assert_eq!(
            counter.used(),
            U64_MAX_AS_U512 * 3 + U512::from(u32::max_value())
        );
        assert!(counter.add(1).is_err());
        assert!(counter.add(u64::max_value()).is_err());
    }

    #[test]
    fn should_mix_gas_amounts() {
        let limit = (U64_MAX_AS_U512 * U512::from(3)) + U512::from(2);
        // let gas_cost = (U64_MAX_AS_U512 * U512::from(3)) + U512::from(1);
        let mut counter = LargeGasCounter::new(Gas::new(limit), Gas::default());
        counter.add(2).unwrap();
        counter.add_large(U64_MAX_AS_U512 * U512::from(3)).unwrap();
        {
            let mut counter = counter;
            assert!(counter.add_large(U512::one()).is_err());
            assert_eq!(counter.used(), limit);
        }
        {
            let mut counter = counter;
            assert!(counter.add(1).is_err());
            assert_eq!(counter.used(), limit);
        }
    }

    #[test]
    fn should_add_large_gas() {
        let limit = (U64_MAX_AS_U512 * U512::from(3)) + U512::from(2);
        let gas_cost = (U64_MAX_AS_U512 * U512::from(3)) + U512::from(1);
        let mut counter = LargeGasCounter::new(Gas::new(limit), Gas::default());
        counter.add_large(gas_cost).unwrap();
        {
            let mut counter = counter;
            counter.add(1).unwrap();
            assert_eq!(counter.used(), limit);
        }

        {
            let mut counter = counter;
            counter.add_large(U512::one()).unwrap();
            assert_eq!(counter.used(), limit);
        }
        assert!(counter.add_large(U64_MAX_AS_U512).is_err());
    }

    #[test]
    fn should_saturate_large_counter() {
        let gas_limit = U64_MAX_AS_U512;
        let mut large_counter = LargeGasCounter::new(Gas::new(gas_limit), Gas::new(U512::zero()));
        assert!(large_counter.add_large(gas_limit + U512::one()).is_err());
        assert_eq!(large_counter.used(), gas_limit);
    }
}
