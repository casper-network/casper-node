use crate::{
    error, security_badge,
    traits::{CEP18Dispatch, CEP18Ref, CEP18State, CEP18},
};
use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, selector, CasperABI, CasperSchema, Contract};
use casper_sdk::{
    collections::{Map, Set},
    host, log, revert,
    schema::CasperSchema,
    types::Address,
    Contract,
};
use error::Cep18Error;
use security_badge::SecurityBadge;
use std::string::String;

#[derive(Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
#[casper(impl_traits(CEP18))]
pub struct TokenContract {
    state: CEP18State,
}

impl Default for TokenContract {
    fn default() -> Self {
        Self {
            state: CEP18State::new("Default name", "Default symbol", 8, 0),
        }
    }
}

#[casper(contract)]
impl TokenContract {
    #[casper(constructor)]
    pub fn new(token_name: String) -> Self {
        // TODO: If argument has same name as another entrypoint there's a compile error for some
        // reason, so can't use "name"
        let mut instance = Self::default();
        instance.state.name = token_name;
        instance.state.enable_mint_burn = true;
        instance
            .state
            .security_badges
            .insert(&host::get_caller(), &SecurityBadge::Admin);
        instance
    }

    pub fn my_balance(&self) -> u64 {
        self.state()
            .balances
            .get(&host::get_caller())
            .unwrap_or_default()
    }
}

impl CEP18 for TokenContract {
    fn state(&self) -> &CEP18State {
        &self.state
    }

    fn state_mut(&mut self) -> &mut CEP18State {
        &mut self.state
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, thread::current};

    use crate::security_badge::SecurityBadge;

    use super::*;

    use casper_sdk::{
        host::{
            self,
            native::{current_environment, with_current_environment, Environment, DEFAULT_ADDRESS},
        },
        types::Address,
        ContractHandle, ContractRef,
    };

    // use traits::CEP18Ref;
    use casper_macros;
    const ALICE: Address = [1; 32];
    const BOB: Address = [2; 32];

    #[test]
    fn it_works() {
        let stub = Environment::new(Default::default(), DEFAULT_ADDRESS);

        let result = host::native::dispatch_with(stub, || {
            let mut contract = TokenContract::new("Foo Token".to_string());

            assert_eq!(contract.state().sec_check(&[SecurityBadge::Admin]), Ok(()));

            assert_eq!(contract.name(), "Foo Token");
            assert_eq!(contract.balance_of(ALICE), 0);
            assert_eq!(contract.balance_of(BOB), 0);

            contract.approve(BOB, 111).unwrap();
            assert_eq!(contract.balance_of(ALICE), 0);
            contract.mint(ALICE, 1000).unwrap();
            assert_eq!(contract.balance_of(ALICE), 1000);

            // DEFAULT_ADDRESS -> ALICE - not much balance
            assert_eq!(contract.balance_of(host::get_caller()), 0);
            assert_eq!(
                contract.transfer(ALICE, 1),
                Err(Cep18Error::InsufficientBalance)
            );
        });
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn e2e() {
        let db = host::native::Container::default();
        let env = Environment::new(db.clone(), DEFAULT_ADDRESS);

        let result = host::native::dispatch_with(env, move || {
            let constructor = TokenContractRef::new("Foo Token".to_string());
            let cep18_handle = TokenContract::create(constructor).expect("Should create");

            {
                // As a builder that allows you to specify value to pass etc.
                cep18_handle
                    .build_call()
                    .cast::<CEP18Ref>()
                    .with_value(1234)
                    .call(|cep18| cep18.name())
                    .expect("Should call");
            }

            let name1: String = cep18_handle
                .build_call()
                .cast::<CEP18Ref>()
                .call(|cep18| cep18.name())
                .expect("Should call");

            let name2: String = cep18_handle
                .build_call()
                .cast::<CEP18Ref>()
                .call(|cep18| cep18.name())
                .expect("Should call");

            assert_eq!(name1, name2);
            assert_eq!(name2, "Foo Token");
            let symbol: String = cep18_handle
                .build_call()
                .cast::<CEP18Ref>()
                .call(|cep18| cep18.symbol())
                .expect("Should call");
            assert_eq!(symbol, "Default symbol");

            let alice_balance: u64 = cep18_handle
                .build_call()
                .cast::<CEP18Ref>()
                .call(|cep18| cep18.balance_of(ALICE))
                .expect("Should call");
            assert_eq!(alice_balance, 0);

            let bob_balance: u64 = cep18_handle
                .build_call()
                .cast::<CEP18Ref>()
                .call(|cep18| cep18.balance_of(BOB))
                .expect("Should call");
            assert_eq!(bob_balance, 0);

            let _mint_succeed: () = cep18_handle
                .build_call()
                .cast::<CEP18Ref>()
                .call(|cep18| cep18.mint(ALICE, 1000))
                .expect("Should succeed")
                .expect("Mint succeeded");

            let alice_balance_after: u64 = cep18_handle
                .build_call()
                .cast::<CEP18Ref>()
                .call(|cep18| cep18.balance_of(ALICE))
                .expect("Should call");
            assert_eq!(alice_balance_after, 1000);

            // Default account -> ALICE
            assert_eq!(
                cep18_handle
                    .build_call()
                    .cast::<CEP18Ref>()
                    .call(|cep18| cep18.transfer(ALICE, 1))
                    .expect("Should call"),
                Err(Cep18Error::InsufficientBalance)
            );
            assert_eq!(host::get_caller(), DEFAULT_ADDRESS);

            let alice_env = current_environment().with_caller(ALICE);

            host::native::dispatch_with(alice_env, || {
                assert_eq!(host::get_caller(), ALICE);
                assert_eq!(
                    cep18_handle
                        .call(|cep18| cep18.my_balance())
                        .expect("Should call"),
                    1000
                );
                assert_eq!(
                    cep18_handle
                        .build_call()
                        .cast::<CEP18Ref>()
                        .call(|cep18| cep18.transfer(BOB, 1))
                        .expect("Should call"),
                    Ok(())
                );
            })
            .expect("Success");

            let bob_balance = cep18_handle
                .build_call()
                .cast::<CEP18Ref>()
                .call(|cep18| cep18.balance_of(BOB))
                .expect("Should call");
            assert_eq!(bob_balance, 1);

            let alice_balance = cep18_handle
                .build_call()
                .cast::<CEP18Ref>()
                .call(|cep18| cep18.balance_of(ALICE))
                .expect("Should call");
            assert_eq!(alice_balance, 999);
        });

        assert_eq!(result, Ok(()));
    }
}
