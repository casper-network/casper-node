#![cfg_attr(target_family = "wasm", no_main)]
const MODULE_PATH: &str = module_path!();

pub mod exports {
    use casper_macros::casper;
    use casper_sdk::{log, types::Address, Contract, ContractHandle};

    use vm2_cep18::contract::TokenContractRef;
    use vm2_cep18::traits::CEP18Ext;

    #[casper(export)]
    pub fn call(address: Address) -> String {
        log!("Hello {address:?}");
        let handle = ContractHandle::<TokenContractRef>::from_address(address);

        // Mint tokens, then check the balance of the account that called this contract
        handle
            .call(|contract| contract.mint([99; 32], 100))
            .expect("Should call")
            .expect("Should mint");

        let balance_result = handle
            .call(|contract| contract.balance_of([99; 32]))
            .expect("Should call");

        assert_eq!(balance_result, 100);

        let name_result = handle
            .call(|contract| contract.name())
            .expect("Should call");
        log!("Name: {name_result:?}");
        name_result
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use casper_sdk::host::native::{self, dispatch_with, Environment};

    #[test]
    fn unit() {
        super::exports::call([42; 32]);
    }

    #[test]
    fn integration() {
        let env = Environment::default().with_input_data(([42u8; 32],));

        let _ = dispatch_with(env, || {
            native::call_export_by_module(super::MODULE_PATH, "call");
        });
    }
}
