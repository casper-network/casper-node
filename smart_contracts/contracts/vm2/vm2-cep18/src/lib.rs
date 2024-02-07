pub mod contract;
pub mod error;
pub mod security_badge;

use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, selector};
use casper_sdk::{log, Contract};
use contract::{CEP18_new, CEP18};

#[casper(export)]
pub fn call() {
    let result = CEP18::create(CEP18_new {
        token_name: "my token name".to_string(),
    })
    .unwrap();
    log!("CEP18 succeeded: {:?}", result);
}

#[cfg(test)]
mod tests {
    use casper_sdk::host::native::{dispatch_with, Stub};

    #[test]
    fn call_should_work() {
        let _ = dispatch_with(Stub::default(), || {
            super::call(&[]);
        });
    }
}
