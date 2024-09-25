#[macro_use]
extern crate alloc;

pub mod contract;
pub mod error;
pub mod traits;

use casper_macros::casper;
use casper_sdk::{log, Contract};
use contract::{NFTContract, NFTContractRef};

#[casper(export)]
pub fn call() {
    log!("Hello");
    let constructor = NFTContractRef::new("my token name".to_string(), 100);
    let _result = NFTContract::create(0, constructor);
    log!("CEP78 succeeded");
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use casper_sdk::host::native::{dispatch_with, Environment};

    #[test]
    fn call_should_work() {
        let _ = dispatch_with(Environment::default(), || {
            super::call();
        });
    }
}
