#![cfg_attr(target_arch = "wasm32", no_main)]

pub mod contracts;
pub mod traits;

#[macro_use]
extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, CasperABI, CasperSchema, Contract};
use casper_sdk::{
    collections::Map,
    host::{self, Entity},
    log, revert,
    types::{Address, CallError},
    Contract, ContractHandle,
};

use contracts::{
    no_fallback,
    token_owner::{self, TokenOwnerContract, TokenOwnerContractRef},
};

fn next_test(counter: &mut u32, name: &str) -> u32 {
    let current = *counter;
    log!("Test {}. Running test: {name}", current);
    *counter += 1;
    current
}

#[casper(export)]
pub fn call() {
    use casper_sdk::ContractBuilder;
    use contracts::harness::{CustomError, INITIAL_GREETING};

    use crate::contracts::{
        harness::{Harness, HarnessRef},
        no_fallback::{NoFallback, NoFallbackRef},
        token_owner::FallbackHandler,
    };

    log!("calling create");

    let session_caller = host::get_caller();
    assert_ne!(session_caller, Entity::Account([0; 32]));

    // Constructor without args
    let mut counter = 1;

    {
        next_test(&mut counter, "Traps and reverts");

        let contract_handle = Harness::create(0, HarnessRef::initialize()).expect("Should create");
        log!("success");
        log!("contract_address: {:?}", contract_handle.contract_address());

        // Verify that the address captured inside constructor is not the same as caller.
        let greeting_result = contract_handle
            .call(|harness| harness.get_greeting())
            .expect("Should call");
        log!("Getting greeting: {greeting_result}");
        assert_eq!(greeting_result, INITIAL_GREETING);

        let () = contract_handle
            .call(|harness| harness.set_greeting("Foo".into()))
            .expect("Should call");

        log!("New greeting saved");
        let greeting_result = contract_handle
            .call(|harness| harness.get_greeting())
            .expect("Should call");
        assert_eq!(greeting_result, "Foo");

        log!("Emitting unreachable trap");

        let call_result = contract_handle.call(|harness| harness.emit_unreachable_trap());
        assert_eq!(call_result, Err(CallError::CalleeTrapped));

        log!("Trap recovered");

        {
            let counter_value_before = contract_handle
                .call(|harness| harness.counter())
                .expect("Should call");

            // increase counter
            let () = contract_handle
                .call(|harness| harness.increment_counter())
                .expect("Should call");

            let counter_value_after = contract_handle
                .call(|harness| harness.counter())
                .expect("Should call");

            assert_eq!(counter_value_before + 1, counter_value_after);
        }

        {
            let counter_value_before = contract_handle
                .call(|harness| harness.counter())
                .expect("Should call");

            let call_result = contract_handle
                .try_call(|harness| harness.emit_revert_with_data())
                .expect("Call succeed");

            assert_eq!(call_result.result, Err(CallError::CalleeReverted));
            assert_eq!(call_result.into_result().unwrap(), Err(CustomError::Bar),);

            let counter_value_after = contract_handle
                .call(|harness| harness.counter())
                .expect("Should call");

            assert_eq!(counter_value_before, counter_value_after);
        }

        log!("Revert with data success");

        let call_result = contract_handle
            .try_call(|harness| harness.emit_revert_without_data())
            .expect("Call succeed");
        assert_eq!(call_result.result, Err(CallError::CalleeReverted));
        assert_eq!(call_result.data, None);

        log!("Revert without data success");

        let call_result = contract_handle
            .try_call(|harness| harness.should_revert_on_error(false))
            .expect("Call succeed");
        assert!(!call_result.did_revert());
        assert_eq!(call_result.into_result().unwrap(), Ok(()));

        log!("Revert on error success (ok case)");

        let call_result = contract_handle
            .try_call(|harness| harness.should_revert_on_error(true))
            .expect("Call succeed");
        assert!(call_result.did_revert());
        assert_eq!(
            call_result.into_result().unwrap(),
            Err(CustomError::WithBody("Reverted".to_string()))
        );

        log!("Revert on error success (err case)");
        // let should_revert_on_error: TypedCall<(bool,), Result<(), CustomError>> =
        //     TypedCall::new(contract_address, selector!("should_revert_on_error"));
        // let result = should_revert_on_error.call((false,));
        // assert!(!result.did_revert());

        // let result = should_revert_on_error.call((true,));
        // assert!(result.did_revert());
        // assert_eq!(
        //     result.into_return_value(),
        //     Err(CustomError::WithBody("Reverted".to_string()))
        // );
    }

    // Constructor with args

    {
        next_test(&mut counter, "Constructor with args");

        let contract_handle = Harness::create(0, HarnessRef::constructor_with_args("World".into()))
            .expect("Should create");
        log!("success 2");
        log!("contract_address: {:?}", contract_handle.contract_address());

        let result = contract_handle
            .call(|harness| harness.get_greeting())
            .expect("Should call");
        assert_eq!(result, "Hello, World!".to_string(),);
    }

    {
        next_test(&mut counter, "Failing constructor");

        let error = Harness::create(0, HarnessRef::failing_constructor("World".to_string()))
            .expect_err(
                "
        Constructor that reverts should fail to create",
            );
        assert_eq!(error, CallError::CalleeReverted);

        let error = Harness::create(0, HarnessRef::trapping_constructor())
            .expect_err("Constructor that traps should fail to create");
        assert_eq!(error, CallError::CalleeTrapped);
    }

    //
    // Check payable entrypoints
    //

    {
        next_test(&mut counter, "Checking payable entrypoints");

        let contract_handle = ContractBuilder::<Harness>::new()
            .with_value(1)
            .create(|| HarnessRef::payable_constructor())
            .expect("Should create");
        Harness::create(0, HarnessRef::constructor_with_args("Payable".to_string()))
            .expect("Should create");
        assert_eq!(contract_handle.balance(), 1);

        log!("success 2");
        log!("contract_address: {:?}", contract_handle.contract_address());

        // Transferring 500 motes before payable entrypoint is executed

        let result_1 = contract_handle
            .build_call()
            .with_value(500)
            .call(|harness| harness.payable_entrypoint())
            .expect("Should call");
        assert_eq!(result_1, Ok(()));

        // Transferring 499 motes before payable entrypoint is executed

        let result_2 = contract_handle
            .build_call()
            .with_value(499)
            .call(|harness| harness.payable_entrypoint())
            .expect("Should call");
        assert_eq!(result_2, Ok(()));

        // Check balance after payable constructor and two successful calls
        assert_eq!(contract_handle.balance(), 1 + 500 + 499);

        let result_3 = contract_handle
            .build_call()
            .with_value(123)
            .call(|harness| harness.payable_failing_entrypoint())
            .expect("Should call");
        assert_eq!(result_3, Err(CustomError::Foo));
        // Check balance after failed call, should be the same as before
        assert_eq!(contract_handle.balance(), 1 + 500 + 499);
    }

    // Deposit and withdraw
    // 1. wasm (caller = A, callee = B)
    //   2. create (caller = B, callee = C)
    //   3. call (caller = B, callee = C)
    //     4. create (caller = C, callee = D)
    //     5. call (caller = C, callee = D)

    {
        let current_test = next_test(&mut counter, "Deposit and withdraw");

        let contract_handle = ContractBuilder::<Harness>::new()
            .with_value(0)
            .create(|| HarnessRef::payable_constructor())
            .expect("Should create");

        let caller = host::get_caller();

        {
            next_test(
                &mut counter,
                &format!("{current_test} Depositing as an account"),
            );
            let account_balance_1 = host::get_balance_of(&caller);
            contract_handle
                .build_call()
                .with_value(100)
                .call(|harness| harness.deposit(account_balance_1))
                .expect("Should call")
                .expect("Should succeed");
            let account_balance_2 = host::get_balance_of(&caller);
            assert_eq!(account_balance_2, account_balance_1 - 100);

            contract_handle
                .build_call()
                .with_value(25)
                .call(|harness| harness.deposit(account_balance_2))
                .expect("Should call")
                .expect("Should succeed");

            let account_balance_after = host::get_balance_of(&caller);
            assert_eq!(account_balance_after, account_balance_1 - 125);
        }

        let current_contract_balance = contract_handle
            .build_call()
            .call(|harness| harness.balance())
            .expect("Should call");
        assert_eq!(current_contract_balance, 100 + 25);

        {
            next_test(
                &mut counter,
                &format!("{current_test} Withdrawing as an account"),
            );
            let account_balance_before = host::get_balance_of(&caller);
            contract_handle
                .build_call()
                .call(|harness| harness.withdraw(account_balance_before, 50))
                .expect("Should call")
                .expect("Should succeed");
            let account_balance_after = host::get_balance_of(&caller);
            assert_ne!(account_balance_after, account_balance_before);
            assert_eq!(account_balance_after, account_balance_before + 50);

            let current_deposit_balance = contract_handle
                .build_call()
                .call(|harness| harness.balance())
                .expect("Should call");
            assert_eq!(current_deposit_balance, 100 + 25 - 50);

            assert_eq!(contract_handle.balance(), 100 + 25 - 50);
        }
    }

    //
    // Perform tests with a contract acting as an owner of funds deposited into other contract
    //

    {
        next_test(
            &mut counter,
            "Contract acts as owner of funds deposited into other contract",
        );

        let caller = host::get_caller();

        let harness = ContractBuilder::<Harness>::new()
            .with_value(0)
            .create(|| HarnessRef::constructor_with_args("Contract".into()))
            .expect("Should create");

        let initial_balance = 1000;

        let token_owner = ContractBuilder::<TokenOwnerContract>::new()
            .with_value(initial_balance)
            .create(|| TokenOwnerContractRef::initialize())
            .expect("Should create");
        assert_eq!(token_owner.balance(), initial_balance);

        // token owner contract performs a deposit into a harness contract through `deposit` payable entrypoint
        // caller: no change
        // token owner: -50
        // harness: +50
        {
            next_test(&mut counter, "Subtest 1");
            let caller_balance_before = host::get_balance_of(&caller);
            let token_owner_balance_before = token_owner.balance();
            let harness_balance_before = harness.balance();

            let initial_deposit = 500;

            token_owner
                .call(|contract| {
                    contract.do_deposit(
                        token_owner.contract_address(),
                        harness.contract_address(),
                        initial_deposit,
                    )
                })
                .expect("Should call")
                .expect("Should succeed");

            assert_eq!(
                host::get_balance_of(&caller),
                caller_balance_before,
                "Caller funds should not change"
            );
            assert_eq!(
                token_owner.balance(),
                token_owner_balance_before - initial_deposit,
                "Token owner balance should decrease"
            );
            assert_eq!(harness.balance(), harness_balance_before + initial_deposit);
        }

        // token owner contract performs a withdrawal from a harness contract through `withdraw` entrypoint
        // caller: no change
        // token owner: +50
        // harness: -50
        {
            next_test(&mut counter, "Subtest 2");
            let caller_balance_before = host::get_balance_of(&caller);
            let token_owner_balance_before = token_owner.balance();
            let harness_balance_before = harness.balance();

            token_owner
                .call(|contract| {
                    contract.do_withdraw(
                        token_owner.contract_address(),
                        harness.contract_address(),
                        50,
                    )
                })
                .expect("Should call")
                .expect("Should succeed");

            assert_eq!(
                host::get_balance_of(&caller),
                caller_balance_before,
                "Caller funds should not change"
            );
            assert_eq!(
                token_owner.balance(),
                token_owner_balance_before + 50,
                "Token owner balance should increase"
            );
            assert_eq!(harness.balance(), harness_balance_before - 50);
            let total_received_tokens = token_owner
                .call(|contract| contract.total_received_tokens())
                .expect("Should call");
            assert_eq!(total_received_tokens, 50);
        }

        {
            next_test(
                &mut counter,
                "Token owner will revert inside fallback while plain transfer",
            );
            {
                let harness_balance_before = harness.balance();
                token_owner
                    .call(|contract| {
                        contract.set_fallback_handler(FallbackHandler::RejectWithRevert)
                    })
                    .expect("Should call");
                let harness_balance_after = harness.balance();
                assert_eq!(harness_balance_before, harness_balance_after);
            }

            {
                let harness_balance_before = harness.balance();
                let withdraw_result = token_owner
                    .call(|contract| {
                        contract.do_withdraw(
                            token_owner.contract_address(),
                            harness.contract_address(),
                            50,
                        )
                    })
                    .expect("Should call");
                let harness_balance_after = harness.balance();
                assert_eq!(harness_balance_before, harness_balance_after);
            }
        }

        {
            next_test(
                &mut counter,
                "Token owner will trap inside fallback while plain transfer",
            );
            {
                let harness_balance_before = harness.balance();
                token_owner
                    .call(|contract| contract.set_fallback_handler(FallbackHandler::RejectWithTrap))
                    .expect("Should call");
                let harness_balance_after = harness.balance();
                assert_eq!(harness_balance_before, harness_balance_after);
            }

            {
                let harness_balance_before = harness.balance();
                let withdraw_result = token_owner
                    .call(|contract| {
                        contract.do_withdraw(
                            token_owner.contract_address(),
                            harness.contract_address(),
                            50,
                        )
                    })
                    .expect("Should call");
                let harness_balance_after = harness.balance();
                assert_eq!(harness_balance_before, harness_balance_after);
            }
        }

        {
            next_test(
                &mut counter,
                "Token owner will revert with data inside fallback while plain transfer",
            );
            {
                let harness_balance_before = harness.balance();
                token_owner
                    .call(|contract| {
                        contract.set_fallback_handler(FallbackHandler::RejectWithData(vec![
                            1, 2, 3, 4, 5,
                        ]))
                    })
                    .expect("Should call");
                let harness_balance_after = harness.balance();
                assert_eq!(harness_balance_before, harness_balance_after);
            }

            {
                let harness_balance_before = harness.balance();
                let withdraw_result = token_owner
                    .call(|contract| {
                        contract.do_withdraw(
                            token_owner.contract_address(),
                            harness.contract_address(),
                            50,
                        )
                    })
                    .expect("Should call");
                let harness_balance_after = harness.balance();
                assert_eq!(harness_balance_before, harness_balance_after);
            }
        }
    }

    {
        let current_test = next_test(
            &mut counter,
            "Plain transfer to a contract does not work without fallback",
        );

        let no_fallback_contract = ContractBuilder::<NoFallback>::new()
            .with_value(0)
            .create(|| NoFallbackRef::initialize())
            .expect("Should create");

        assert_eq!(
            host::casper_transfer(&no_fallback_contract.entity(), 123),
            Err(CallError::NotCallable)
        );
    }

    log!("👋 Goodbye");
}

#[cfg(test)]
mod tests {

    use alloc::collections::{BTreeMap, BTreeSet};

    use casper_macros::selector;
    use casper_sdk::{
        host::native::{self, dispatch},
        schema::CasperSchema,
    };
    use vm_common::{flags::EntryPointFlags, selector::Selector};

    use super::*;

    #[test]
    fn trait_has_interface() {
        let interface = FallbackRef::SELECTOR;
        assert_eq!(interface, Selector::zero());
    }

    #[test]
    fn fallback_trait_flags_selectors() {
        let mut value = 0;

        for entry_point in HarnessRef::ENTRY_POINTS.iter().map(|e| *e).flatten() {
            if entry_point.flags == EntryPointFlags::FALLBACK.bits() {
                value ^= entry_point.selector;
            }
        }

        assert_eq!(value, 0);
    }

    #[test]
    fn test() {
        dispatch(|| {
            native::call_export("call");
        })
        .unwrap();
    }

    #[test]
    fn exports() {
        let exports = native::list_exports()
            .into_iter()
            .map(|e| e.name)
            .collect::<Vec<_>>();
        assert_eq!(exports, vec!["call"]);
    }

    #[test]
    fn compile_time_schema() {
        let schema = Harness::schema();
        dbg!(&schema);
        // println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }

    #[test]
    fn should_greet() {
        assert_eq!(Harness::name(), "Harness");
        let mut flipper = Harness::constructor_with_args("Hello".into());
        assert_eq!(flipper.get_greeting(), "Hello"); // TODO: Initializer
        flipper.set_greeting("Hi".into());
        assert_eq!(flipper.get_greeting(), "Hi");
    }

    #[test]
    fn unittest() {
        dispatch(|| {
            let mut foo = Harness::initialize();
            assert_eq!(foo.get_greeting(), INITIAL_GREETING);
            foo.set_greeting("New greeting".to_string());
            assert_eq!(foo.get_greeting(), "New greeting");
        })
        .unwrap();
    }

    #[test]
    fn list_of_constructors() {
        let schema = Harness::schema();
        let constructors: BTreeSet<_> = schema
            .entry_points
            .iter()
            .filter_map(|e| {
                if e.flags.contains(EntryPointFlags::CONSTRUCTOR) {
                    Some(e.name.as_str())
                } else {
                    None
                }
            })
            .collect();
        let expected = BTreeSet::from_iter([
            "constructor_with_args",
            "failing_constructor",
            "trapping_constructor",
            "initialize",
        ]);

        assert_eq!(constructors, expected);
    }

    #[test]
    fn check_schema_selectors() {
        let schema = Harness::schema();

        let schema_mapping: BTreeMap<_, _> = schema
            .entry_points
            .iter()
            .map(|e| (e.name.as_str(), e.selector))
            .collect();

        let manifest = &Harness::MANIFEST;

        let manifest_selectors: BTreeSet<u32> = manifest.iter().map(|e| e.selector).collect();

        assert_eq!(schema_mapping["constructor_with_args"], Some(4116419170),);
        assert!(manifest_selectors.contains(&4116419170));
    }

    #[test]
    fn verify_check_private_and_public_methods() {
        dispatch(|| {
            Harness::default().private_function_that_should_not_be_exported();
            Harness::default().restricted_function_that_should_be_part_of_manifest();
        })
        .expect("No trap");

        let manifest = &Harness::MANIFEST;
        const PRIVATE_SELECTOR: Selector =
            selector!("private_function_that_should_not_be_exported");
        const PUB_CRATE_SELECTOR: Selector =
            selector!("restricted_function_that_should_be_part_of_manifest");
        assert!(
            manifest
                .iter()
                .find(|e| e.selector == PRIVATE_SELECTOR.get())
                .is_none(),
            "This entry point should not be part of manifest"
        );
        assert!(
            manifest
                .iter()
                .find(|e| e.selector == PUB_CRATE_SELECTOR.get())
                .is_some(),
            "This entry point should be part of manifest"
        );

        let schema = Harness::schema();
        assert!(
            schema
                .entry_points
                .iter()
                .find(|e| e.selector == Some(PRIVATE_SELECTOR.get()))
                .is_none(),
            "This entry point should not be part of schema"
        );
        assert!(
            schema
                .entry_points
                .iter()
                .find(|e| e.selector == Some(PUB_CRATE_SELECTOR.get()))
                .is_some(),
            "This entry point should be part ozf schema"
        );
    }

    #[test]
    fn foo() {
        assert_eq!(Harness::default().into_greeting(), "Default value");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    panic!("Execute \"cargo test\" to test the contract, \"cargo build\" to build it");
}
