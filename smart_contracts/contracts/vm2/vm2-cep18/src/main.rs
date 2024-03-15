#![cfg_attr(target_family = "wasm", no_main)]
use casper_sdk::cli;
use vm2_cep18::contract::{TokenContract, TokenContractRef};

use casper_macros::casper;
use casper_sdk::{log, Contract};

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

#[cfg(not(target_family = "wasm"))]
fn main() {
    cli::command_line::<TokenContract>();
}
