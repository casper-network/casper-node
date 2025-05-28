use casper_contract_macros::casper;
use casper_contract_sdk::casper;

/// A contract that can't receive tokens through a plain `fallback` method.
#[derive(Default)]
#[casper(contract_state)]
pub struct NoFallback {
    initial_balance: u64,
    received_balance: u64,
}

#[casper]
impl NoFallback {
    #[casper(constructor)]
    pub fn no_fallback_initialize() -> Self {
        Self {
            initial_balance: casper::transferred_value(),
            received_balance: 0,
        }
    }

    pub fn hello(&self) -> &str {
        "Hello, World!"
    }

    #[casper(payable)]
    pub fn receive_funds(&mut self) {
        let value = casper::transferred_value();
        self.received_balance += value;
    }
}
