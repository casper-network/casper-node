#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_sdk::prelude::*;

#[casper(contract_state)]
pub struct Contract {
    counter: u64,
}

impl Default for Contract {
    fn default() -> Self {
        panic!("Unable to instantiate contract without a constructor!");
    }
}

#[casper]
impl Contract {
    #[casper(constructor)]
    pub fn new() -> Self {
        Self { counter: 0 }
    }

    #[casper(constructor)]
    pub fn default() -> Self {
        Self::new()
    }

    pub fn increase(&mut self) {
        self.counter += 1;
    }

    pub fn get(&self) -> u64 {
        self.counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let mut counter = Contract::new();
        assert_eq!(counter.get(), 0);
        counter.increase();
        assert_eq!(counter.get(), 1);
        counter.increase();
        assert_eq!(counter.get(), 2);
    }
}
