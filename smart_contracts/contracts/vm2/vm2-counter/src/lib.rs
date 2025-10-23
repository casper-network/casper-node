#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_sdk::prelude::*;

/// This contract implements a simple counter.
#[casper(contract_state)]
#[derive(Default)]
pub struct Counter {
    /// The current state of the counter.
    value: u32,
}

#[casper]
impl Counter {
    #[casper(constructor)]
    pub fn new(init_value: u32) -> Self {
        Self { value: init_value }
    }

    #[casper(constructor)]
    pub fn default() -> Self {
        Self::new(Default::default())
    }

    pub fn increment(&mut self) {
        self.value = self.value.saturating_add(1);
    }

    pub fn decrement(&mut self) {
        self.value = self.value.saturating_sub(1);
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
        const INIT: u32 = 0;
        let mut counter = Counter::new(INIT);
        assert_eq!(counter.get(), INIT);
        counter.increment();
        assert_eq!(counter.get(), INIT + 1u32);
        counter.increment();
        assert_eq!(counter.get(), INIT + 2u32);
        counter.decrement();
        assert_eq!(counter.get(), INIT + 1u32);
        counter.decrement();
        assert_eq!(counter.get(), INIT);
        counter.decrement(); // saturating sub
        assert_eq!(counter.get(), INIT);
    }
}
