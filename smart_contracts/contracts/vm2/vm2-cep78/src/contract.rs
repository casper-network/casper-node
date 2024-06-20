use crate::{
    error::NFTCoreError,
    traits::{CEP18Ref, CEP78State, CEP18},
};
use casper_macros::casper;
use casper_sdk::{
    host,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};
use std::string::String;

use crate::traits::CEP18Ext;

#[derive(Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
#[borsh(crate = "casper_sdk::serializers::borsh")]
#[casper(impl_traits(CEP18))]
pub struct NFTContract {
    state: CEP78State,
}

impl Default for NFTContract {
    fn default() -> Self {
        panic!("The contract is not initialized");
    }
}

#[casper(contract)]
impl NFTContract {
    #[casper(constructor)]
    pub fn new(collection_name: String, total_token_supply: u64) -> NFTContract {
        // TODO: Does host needs better support for aborting execution? Currently, this would be
        // seen as a trapped execution with a possible casper_print call coming from a panic
        // handler. let state =
        //     CEP78State::new(collection_name, total_token_supply);
        // Self { state }
        todo!()
    }
}

impl CEP18 for NFTContract {
    fn state(&self) -> &CEP78State {
        &self.state
    }

    fn state_mut(&mut self) -> &mut CEP78State {
        &mut self.state
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = host::native::dispatch(|| {
            let contract = NFTContract::new("Foo NFT".to_string(), 35);
            assert_eq!(contract.state().collection_name, "Foo NFT");
        });
        assert_eq!(result, Ok(()));
    }
}
