#![cfg_attr(target_family = "wasm", no_main)]

pub mod exports {
    use casper_contract_sdk::{
        contrib::cep18::{CEP18Ext, MintableExt},
        prelude::*,
        types::{Address, U256},
        ContractHandle,
    };
    use vm2_cep18::TokenContractRef;

    #[casper(export)]
    pub fn call(address: Address) -> String {
        use casper_contract_sdk::casper::Entity;

        log!("Hello {address:?}");
        let handle = ContractHandle::<TokenContractRef>::from_address(address);

        // Mint tokens, then check the balance of the account that called this contract
        handle
            .call(|contract| contract.mint(Entity::Account([99; 32]), U256::from(100u64)))
            .expect("Should call")
            .expect("Should mint");

        let balance_result = handle
            .call(|contract| contract.balance_of(Entity::Account([99; 32])))
            .expect("Should call");

        assert_eq!(balance_result, U256::from(100u64));

        let name_result = handle
            .call(|contract| contract.name())
            .expect("Should call");
        log!("Name: {name_result:?}");
        let transfer_result = handle
            .call(|contract| contract.transfer(Entity::Account([100; 32]), U256::from(100u64)))
            .expect("Should call");

        log!("Transfer: {transfer_result:?}");

        log!("Success");

        name_result
    }
}
