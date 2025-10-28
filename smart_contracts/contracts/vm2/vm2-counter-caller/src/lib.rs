use casper_contract_sdk::{prelude::*, types::Address, ContractHandle};
use vm2_counter::CounterRef;

#[casper(contract_state)]
pub struct CounterCallerContract {}

#[casper]
impl CounterCallerContract {
    #[casper(constructor)]
    pub fn create() -> Self {
        Self {}
    }

    pub fn inc_and_get(&self, address: Address) -> u32 {
        let handle = ContractHandle::<CounterRef>::from_address(address);
        // Mint tokens, then check the balance of the account that called this contract
        handle
            .call(|contract| contract.increment())
            .expect("Should call");

        let counter = handle.call(|contract| contract.get()).expect("Should call");

        counter
    }
}

impl Default for CounterCallerContract {
    fn default() -> Self {
        panic!("nope");
    }
}
