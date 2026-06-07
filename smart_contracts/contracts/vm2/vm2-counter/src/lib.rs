#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_sdk::prelude::*;

/// This contract implements a simple counter.
#[casper(contract_state)]
pub struct Counter {
    /// The current counter value.
    value: u32,
}

impl Default for Counter {
    fn default() -> Self {
        panic!("Unable to instantiate contract without a constructor");
    }
}

#[casper]
impl Counter {
    #[casper(constructor)]
    pub fn new(init: u32) -> Self {
        Self { value: init }
    }

    #[casper(constructor)]
    pub fn default() -> Self {
        Self::new(0)
    }

    pub fn increment(&mut self) {
        self.value += 1;
    }

    pub fn get(&self) -> u32 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let mut counter = Counter::new(0);
        assert_eq!(counter.get(), 0);
        counter.increment();
        assert_eq!(counter.get(), 1);
        counter.increment();
        assert_eq!(counter.get(), 2);
    }
}
