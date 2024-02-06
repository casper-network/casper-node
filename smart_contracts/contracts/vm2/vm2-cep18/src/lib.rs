pub mod contract;
pub mod error;
pub mod security_badge;

use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, selector};
use casper_sdk::Contract;
use contract::CEP18;

#[casper(export)]
pub fn call() {
    let args = ("my token name".to_string(),);
    // CEP18::
    let result = CEP18::create(selector!("new"), Some(&borsh::to_vec(&args).unwrap())).unwrap();
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::security_badge::SecurityBadge;

    use super::*;

    use casper_sdk::{
        abi::CasperABI,
        host::{self, native::Stub},
        schema::CasperSchema,
        types::Address,
    };

    const DEFAULT_ACCOUNT: Address = [42; 32];
    const ALICE: Address = [1; 32];
    const BOB: Address = [2; 32];

    #[test]
    fn should_generate_abi() {
        dbg!(CEP18::definition());
        // add verification of ABI
    }

    #[test]
    fn schema() {
        let s = serde_json::to_string_pretty(&CEP18::schema()).unwrap();
        fs::write("/tmp/cep18_schema.json", &s).unwrap();
    }

    #[test]
    fn it_works() {
        let stub = Stub::new(Default::default(), [42; 32]);

        let result = host::native::dispatch_with(stub, || {
            let mut contract = CEP18::new("Foo Token".to_string());

            contract.sec_check(&[SecurityBadge::Admin]).unwrap();

            assert_eq!(contract.name(), "Foo Token");
            assert_eq!(contract.balance_of(ALICE), 0);
            assert_eq!(contract.balance_of(BOB), 0);

            contract.approve(BOB, 111).unwrap();
            assert_eq!(contract.balance_of(ALICE), 0);
            contract.mint(ALICE, 1000).unwrap();
            assert_eq!(contract.balance_of(ALICE), 1000);

            // [42; 32] -> ALICE - not much balance
            assert_eq!(contract.balance_of(host::get_caller()), 0);
            assert_eq!(
                contract.transfer(ALICE, 1),
                Err(Cep18Error::InsufficientBalance)
            );
        });
        assert_eq!(result, Ok(()));
    }
}
