#![cfg_attr(target_arch = "wasm32", no_main)]

use casper_macros::casper;
use casper_sdk::log;

#[casper(export)]
pub fn call() {
    log!("Hello");
    // perform_test();
    log!("🎉 Success");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call() {
        call();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
