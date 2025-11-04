use casper_contract_sdk::prelude::*;

#[casper(contract_state)]
pub struct ReturnerContract {}

impl Default for ReturnerContract {
    fn default() -> Self {
        panic!("nope");
    }
}

#[casper]
impl ReturnerContract {
    #[casper(constructor)]
    pub fn new() -> Self {
        Self {}
    }

    pub fn do_return(&self) -> u32 {
        let _ = casper::print(&format!("returning"));
        123
    }

    pub fn do_not_return(&self) {
        let _ = casper::print("not returning");
    }
}
