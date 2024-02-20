pub mod contract;
pub mod error;
pub mod security_badge;

use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, selector};
use casper_sdk::{log, Contract};
use contract::{CEP18Ref, CEP18};

#[casper(export)]
pub fn call() {
    let constructor = CEP18Ref::new("my token name".to_string());
    let result = CEP18::create(constructor);
    log!("CEP18 succeeded");
}

#[cfg(test)]
mod tests {
    use casper_sdk::host::native::{dispatch_with, Environment};

    #[test]
    fn call_should_work() {
        let _ = dispatch_with(Environment::default(), || {
            super::call(&[]);
        });
    }
}
