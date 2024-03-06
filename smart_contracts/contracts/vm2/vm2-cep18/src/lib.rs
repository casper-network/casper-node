#[macro_use]
extern crate alloc;

pub mod contract;
pub mod error;
pub mod security_badge;
pub mod traits;

use casper_macros::casper;
use casper_sdk::{log, Contract};
use contract::{TokenContract, TokenContractRef};

#[casper(export)]
pub fn call() {
    log!("Hello");
    let constructor = TokenContractRef::new("my token name".to_string());
    let _result = TokenContract::create(constructor);
    log!("CEP18 succeeded");
}

#[cfg(test)]
mod tests {
    use casper_sdk::host::native::dispatch;

    #[test]
    fn call_should_work() {
        let _ = dispatch(|| {
            super::call();
        });
    }
}
