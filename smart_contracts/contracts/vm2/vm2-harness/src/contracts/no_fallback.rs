use casper_macros::casper;
use casper_sdk::host;

/// A contract that can't receive tokens through a plain `fallback` method.
#[derive(Default)]
#[casper(contract_state)]
pub struct NoFallback {
    initial_balance: u128,
    received_balance: u128,
}

#[casper]
impl NoFallback {
    #[casper(constructor)]
    pub fn no_fallback_initialize() -> Self {
        Self {
            initial_balance: host::get_value(),
            received_balance: 0,
        }
    }

    pub fn hello(&self) -> &str {
        "Hello, World!"
    }

    #[casper(payable)]
    pub fn receive_funds(&mut self) {
        let value = host::get_value();
        self.received_balance += value;
    }
}
