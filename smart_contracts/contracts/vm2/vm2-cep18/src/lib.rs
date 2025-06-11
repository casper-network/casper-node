use casper_contract_sdk::{
    contrib::access_control::{AccessControl, AccessControlExt, AccessControlState},
    prelude::*,
    types::U256,
};

use casper_contract_sdk::contrib::cep18::{
    Burnable, BurnableExt, CEP18Ext, CEP18State, Mintable, MintableExt, ADMIN_ROLE, CEP18,
};

#[casper(contract_state)]
pub struct TokenContract {
    state: CEP18State,
    access_control: AccessControlState,
}

impl Default for TokenContract {
    fn default() -> Self {
        panic!("nope");
    }
    //
}

#[casper]
impl TokenContract {
    #[casper(constructor)]
    pub fn new(token_name: String) -> Self {
        // TODO: If argument has same name as another entrypoint there's a compile error for some
        // reason, so can't use "name"
        let mut state = CEP18State::new(&token_name, "Default symbol", 8, U256::from(0u64));
        state.enable_mint_burn = true;

        let mut token = Self {
            state,
            access_control: AccessControlState::default(),
        };

        let caller = casper::get_caller();
        token.grant_role(caller, ADMIN_ROLE);

        // Give caller some tokens
        token.mint(caller, U256::from(10_000u64)).expect("Mint");

        token
    }

    pub fn my_balance(&self) -> U256 {
        CEP18::state(self)
            .balances
            .get(&casper::get_caller())
            .unwrap_or_default()
    }
}

#[casper(path = casper_contract_sdk::contrib::cep18)]
impl CEP18 for TokenContract {
    fn state(&self) -> &CEP18State {
        &self.state
    }

    fn state_mut(&mut self) -> &mut CEP18State {
        &mut self.state
    }
}

#[casper(path = casper_contract_sdk::contrib::access_control)]
impl AccessControl for TokenContract {
    fn state(&self) -> &AccessControlState {
        &self.access_control
    }

    fn state_mut(&mut self) -> &mut AccessControlState {
        &mut self.access_control
    }
}

#[casper(path = casper_contract_sdk::contrib::cep18)]
impl Mintable for TokenContract {}

#[casper(path = casper_contract_sdk::contrib::cep18)]
impl Burnable for TokenContract {}

#[cfg(test)]
mod tests {
    use super::*;

    use casper_contract_sdk::{
        casper::{
            self,
            native::{
                current_environment, dispatch_with, with_current_environment, Environment,
                DEFAULT_ADDRESS,
            },
            Entity,
        },
        casper_executor_wasm_common::keyspace::Keyspace,
        contrib::cep18::Cep18Error,
        ContractHandle, ToCallData,
    };

    const ALICE: Entity = Entity::Account([1; 32]);
    const BOB: Entity = Entity::Account([2; 32]);

    #[test]
    fn it_works() {
        let stub = Environment::new(Default::default(), DEFAULT_ADDRESS);

        let result = casper::native::dispatch_with(stub, || {
            let mut contract = TokenContract::new("Foo Token".to_string());

            assert_eq!(contract.require_any_role(&[ADMIN_ROLE]), Ok(()));

            assert_eq!(contract.name(), "Foo Token");
            assert_eq!(contract.balance_of(ALICE), U256::from(0u64));
            assert_eq!(contract.balance_of(BOB), U256::from(0u64));

            contract.approve(BOB, U256::from(111u64)).unwrap();
            assert_eq!(contract.balance_of(ALICE), U256::from(0u64));
            contract.mint(ALICE, U256::from(1000u64)).unwrap();
            assert_eq!(contract.balance_of(ALICE), U256::from(1000u64));

            // Caller has 10k tokens mintes (coming from constructor)
            assert_eq!(
                contract.balance_of(casper::get_caller()),
                U256::from(10_000u64)
            );
            assert_eq!(
                contract.transfer(ALICE, U256::from(10_001u64)),
                Err(Cep18Error::InsufficientBalance)
            );
            assert_eq!(contract.transfer(ALICE, U256::from(10_000u64)), Ok(()));
        });
        assert!(matches!(result, Ok(())));
    }

    #[test]
    fn e2e() {
        // let db = casper::native::Container::default();
        // let env = Environment::new(db.clone(), DEFAULT_ADDRESS);

        let result = casper::native::dispatch(move || {
            assert_eq!(casper::get_caller(), DEFAULT_ADDRESS);

            let constructor = TokenContractRef::new("Foo Token".to_string());

            // casper_call(address, value, selector!("nme"), ());
            let ctor_input_data = constructor.input_data();
            let create_result = casper::create(
                None,
                0,
                Some(constructor.entry_point()),
                ctor_input_data.as_ref().map(|data| data.as_slice()),
                None,
            )
            .expect("Should create");

            let new_env = with_current_environment(|env| env);
            let new_env = new_env.smart_contract(Entity::Contract(create_result.contract_address));
            dispatch_with(new_env, || {
                // This is the caller of the contract
                casper::read_into_vec(Keyspace::State)
                    .expect("ok")
                    .expect("ok");
            })
            .unwrap();

            // assert_eq!(casper::get_caller(), DEFAULT_ADDRESS);

            let cep18_handle =
                ContractHandle::<TokenContractRef>::from_address(create_result.contract_address);

            {
                // As a builder that allows you to specify value to pass etc.
                cep18_handle
                    .build_call()
                    .with_transferred_value(0)
                    .call(|cep18| cep18.name())
                    .expect("Should call");
            }

            let name1: String = cep18_handle
                .build_call()
                .call(|cep18| cep18.name())
                .expect("Should call");

            let name2: String = cep18_handle
                .build_call()
                .call(|cep18| cep18.name())
                .expect("Should call");

            assert_eq!(name1, name2);
            assert_eq!(name2, "Foo Token");
            let symbol: String = cep18_handle
                .build_call()
                .call(|cep18| cep18.symbol())
                .expect("Should call");
            assert_eq!(symbol, "Default symbol");

            let alice_balance: U256 = cep18_handle
                .build_call()
                .call(|cep18| cep18.balance_of(ALICE))
                .expect("Should call");
            assert_eq!(alice_balance, U256::from(0u64));

            let bob_balance: U256 = cep18_handle
                .build_call()
                .call(|cep18| cep18.balance_of(BOB))
                .expect("Should call");
            assert_eq!(bob_balance, U256::from(0u64));

            let _mint_succeed: () = cep18_handle
                .build_call()
                .call(|cep18| cep18.mint(ALICE, U256::from(1000u64)))
                .expect("Should succeed")
                .expect("Mint succeeded");

            let alice_balance_after: U256 = cep18_handle
                .build_call()
                .call(|cep18| cep18.balance_of(ALICE))
                .expect("Should call");
            assert_eq!(alice_balance_after, U256::from(1000u64));

            // Default account -> ALICE

            let default_addr_balance: U256 = cep18_handle
                .build_call()
                .call(|cep18| cep18.balance_of(DEFAULT_ADDRESS))
                .expect("Should call");
            assert_eq!(default_addr_balance, U256::from(10_000u64));

            assert_eq!(
                cep18_handle
                    .build_call()
                    .call(|cep18| cep18.transfer(ALICE, U256::from(10_001u64)))
                    .expect("Should call"),
                Err(Cep18Error::InsufficientBalance)
            );
            assert_eq!(casper::get_caller(), DEFAULT_ADDRESS);

            let alice_env = current_environment().session(ALICE);

            casper::native::dispatch_with(alice_env, || {
                assert_eq!(casper::get_caller(), ALICE);
                assert_eq!(
                    cep18_handle
                        .call(|cep18| cep18.my_balance())
                        .expect("Should call"),
                    U256::from(1000u64)
                );
                assert_eq!(
                    cep18_handle
                        .build_call()
                        .call(|cep18| cep18.transfer(BOB, U256::from(1u64)))
                        .expect("Should call"),
                    Ok(())
                );
            })
            .expect("Success");

            let bob_balance = cep18_handle
                .build_call()
                .call(|cep18| cep18.balance_of(BOB))
                .expect("Should call");
            assert_eq!(bob_balance, U256::from(1u64));

            let alice_balance = cep18_handle
                .build_call()
                .call(|cep18| cep18.balance_of(ALICE))
                .expect("Should call");
            assert_eq!(alice_balance, U256::from(999u64));
        });

        assert!(matches!(result, Ok(())));
    }
}
