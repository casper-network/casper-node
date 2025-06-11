#![cfg_attr(target_arch = "wasm32", no_main)]

pub mod contracts;
pub mod traits;

#[macro_use]
extern crate alloc;

use casper_contract_macros::casper;
use casper_contract_sdk::{
    casper::{self, emit, emit_raw, Entity},
    casper_executor_wasm_common::{error::CommonResult, keyspace::Keyspace},
    log,
    types::{Address, CallError},
};

use contracts::token_owner::TokenOwnerContractRef;

#[casper(message)]
pub struct TestMessage {
    pub message: String,
}

#[derive(Default)]
struct Seed {
    value: u64,
}

impl Seed {
    fn next_seed(&mut self) -> [u8; 32] {
        let current_value = {
            let mut value: [u8; 32] = Default::default();
            value[32 - 8..].copy_from_slice(&self.value.to_be_bytes());
            value
        };
        self.value += 1;
        current_value
    }
}

fn next_test(counter: &mut u32, name: &str) -> u32 {
    let current = *counter;
    log!("Test {}. Running test: {name}", current);
    *counter += 1;
    current
}

fn perform_test(seed: &mut Seed, flipper_address: Address) {
    use casper_contract_sdk::ContractBuilder;
    use contracts::harness::{CustomError, INITIAL_GREETING};

    use crate::contracts::{harness::HarnessRef, token_owner::FallbackHandler};

    log!("calling create");

    let session_caller = casper::get_caller();
    assert_ne!(session_caller, Entity::Account([0; 32]));

    // Constructor without args
    let mut counter = 1;

    {
        next_test(&mut counter, "Traps and reverts");

        let contract_handle = ContractBuilder::<HarnessRef>::new()
            .with_seed(&seed.next_seed())
            .create(|| HarnessRef::initialize())
            .expect("Should create");
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

        let contract_handle = ContractBuilder::<HarnessRef>::new()
            .with_seed(&seed.next_seed())
            .create(|| HarnessRef::constructor_with_args("World".into()))
            .expect("Should create");
        log!("success 2");
        log!("contract_address: {:?}", contract_handle.contract_address());

        // Calling constructor twice should fail
        let error = match contract_handle
            .try_call(|_| HarnessRef::constructor_with_args("World".into()))
        {
            Ok(_) => panic!("Constructor should fail to initialize already initialized contract"),
            Err(error) => error,
        };
        assert_eq!(error, CallError::CalleeTrapped);

        let result = contract_handle
            .call(|harness| harness.get_greeting())
            .expect("Should call");
        assert_eq!(result, "Hello, World!".to_string(),);
    }

    {
        next_test(&mut counter, "Failing constructor");

        let error = match ContractBuilder::<HarnessRef>::new()
            .with_seed(&seed.next_seed())
            .create(|| HarnessRef::failing_constructor("World".to_string()))
        {
            Ok(_) => panic!("Constructor that reverts should fail to create"),
            Err(error) => error,
        };
        assert_eq!(error, CallError::CalleeReverted);

        let error = match ContractBuilder::<HarnessRef>::new()
            .with_seed(&seed.next_seed())
            .create(|| HarnessRef::trapping_constructor())
        {
            Ok(_) => panic!("Constructor that traps should fail to create"),
            Err(error) => error,
        };
        assert_eq!(error, CallError::CalleeTrapped);
    }

    //
    // Check payable entrypoints
    //

    {
        next_test(&mut counter, "Checking payable entrypoints");

        let contract_handle = ContractBuilder::<HarnessRef>::new()
            .with_transferred_value(1)
            .with_seed(&seed.next_seed())
            .create(|| HarnessRef::payable_constructor())
            .expect("Should create");

        assert_eq!(contract_handle.balance(), 1);

        log!("success 2");
        log!("contract_address: {:?}", contract_handle.contract_address());

        // Transferring 500 motes before payable entrypoint is executed

        let result_1 = contract_handle
            .build_call()
            .with_transferred_value(500)
            .call(|harness| harness.payable_entrypoint())
            .expect("Should call");
        assert_eq!(result_1, Ok(()));

        // Transferring 499 motes before payable entrypoint is executed

        let result_2 = contract_handle
            .build_call()
            .with_transferred_value(499)
            .call(|harness| harness.payable_entrypoint())
            .expect("Should call");
        assert_eq!(result_2, Ok(()));

        // Check balance after payable constructor and two successful calls
        assert_eq!(contract_handle.balance(), 1 + 500 + 499);

        let result_3 = contract_handle
            .build_call()
            .with_transferred_value(123)
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

        let contract_handle = ContractBuilder::<HarnessRef>::new()
            .with_transferred_value(0)
            .with_seed(&seed.next_seed())
            .create(|| HarnessRef::payable_constructor())
            .expect("Should create");

        let caller = casper::get_caller();

        {
            next_test(
                &mut counter,
                &format!("{current_test} Depositing as an account"),
            );
            let account_balance_1 = casper::get_balance_of(&caller);
            contract_handle
                .build_call()
                .with_transferred_value(100)
                .call(|harness| harness.perform_token_deposit(account_balance_1))
                .expect("Should call")
                .expect("Should succeed");
            let account_balance_2 = casper::get_balance_of(&caller);
            assert_eq!(account_balance_2, account_balance_1 - 100);

            contract_handle
                .build_call()
                .with_transferred_value(25)
                .call(|harness| harness.perform_token_deposit(account_balance_2))
                .expect("Should call")
                .expect("Should succeed");

            let account_balance_after = casper::get_balance_of(&caller);
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
            let account_balance_before = casper::get_balance_of(&caller);
            contract_handle
                .build_call()
                .call(|harness| harness.withdraw(account_balance_before, 50))
                .expect("Should call")
                .expect("Should succeed");
            let account_balance_after = casper::get_balance_of(&caller);
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

        let caller = casper::get_caller();

        let harness = ContractBuilder::<HarnessRef>::new()
            .with_transferred_value(0)
            .with_seed(&seed.next_seed())
            .create(|| HarnessRef::constructor_with_args("Contract".into()))
            .expect("Should create");

        let initial_balance = 1000;

        let token_owner = ContractBuilder::<TokenOwnerContractRef>::new()
            .with_transferred_value(initial_balance)
            .with_seed(&seed.next_seed())
            .create(|| TokenOwnerContractRef::token_owner_initialize())
            .expect("Should create");
        assert_eq!(token_owner.balance(), initial_balance);

        // token owner contract performs a deposit into a harness contract through `deposit` payable
        // entrypoint caller: no change
        // token owner: -50
        // harness: +50
        {
            next_test(&mut counter, "Subtest 1");
            let caller_balance_before = casper::get_balance_of(&caller);
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
                casper::get_balance_of(&caller),
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

        // token owner contract performs a withdrawal from a harness contract through `withdraw`
        // entrypoint caller: no change
        // token owner: +50
        // harness: -50
        {
            next_test(&mut counter, "Subtest 2");
            let caller_balance_before = casper::get_balance_of(&caller);
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
                casper::get_balance_of(&caller),
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
                assert_eq!(
                    withdraw_result,
                    Err(
                        crate::contracts::token_owner::TokenOwnerError::WithdrawError(
                            "deposit error callee reverted".to_string()
                        )
                    )
                );
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
                assert_eq!(
                    withdraw_result,
                    Err(
                        crate::contracts::token_owner::TokenOwnerError::WithdrawError(
                            "deposit error callee trapped".to_string()
                        )
                    )
                );
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
                assert_eq!(
                    withdraw_result,
                    Err(
                        crate::contracts::token_owner::TokenOwnerError::WithdrawError(
                            "deposit error callee reverted".to_string()
                        )
                    )
                );
            }
        }
    }

    // {
    //     let _current_test = next_test(
    //         &mut counter,
    //         "Plain transfer to a contract does not work without fallback",
    //     );
    //     let flipper_address = Entity::Contract(flipper_address);

    //     // assert_eq!(
    //     //     host::casper_transfer(&flipper_address, 123),
    //     //     Err(CallError::NotCallable)
    //     // );
    // }

    {
        let _current_test = next_test(
            &mut counter,
            "Calling non-existing entrypoint does not crash",
        );
        let (output, result) =
            casper::casper_call(&flipper_address, 0, "non_existing_entrypoint", &[]);
        assert_eq!(result, Err(CallError::NotCallable));
        assert_eq!(output, None);
    }

    {
        let _current_test = next_test(&mut counter, "Message passing");

        for i in 0..10 {
            assert_eq!(
                emit(TestMessage {
                    message: format!("Hello, world: {i}!"),
                }),
                Ok(())
            );
        }

        let small_topic_name = "a".repeat(32);
        let large_topic_name = "a".repeat(257);
        let large_payload_data = vec![0; 16384];

        assert_eq!(
            emit_raw(&large_topic_name, &[]),
            Err(CommonResult::TopicTooLong)
        );
        assert_eq!(
            emit_raw(&small_topic_name, &large_payload_data),
            Err(CommonResult::PayloadTooLong)
        );

        for i in 0..127u64 {
            assert_eq!(
                emit_raw(&format!("Topic{i}"), &i.to_be_bytes()),
                Ok(()),
                "Emitting message with small payload failed"
            );
        }

        assert_eq!(
            emit_raw(&format!("Topic128"), &[128]),
            Err(CommonResult::TooManyTopics),
            "Emitting message with small payload failed"
        );
    }

    {
        next_test(&mut counter, "Removing from global state");
        let key = [0, 1, 2, 3];
        let value_1 = [4, 5, 6, 7];
        let value_2 = [8, 9, 10, 11, 12, 13, 14, 15];
        let keyspace = Keyspace::Context(&key);
        // No value exists
        assert_eq!(casper::read(keyspace, |_size| None), Ok(None));

        // Write a value
        casper::write(keyspace, &value_1).unwrap();
        // Value exists
        assert_eq!(casper::read_into_vec(keyspace), Ok(Some(value_1.to_vec())));
        // Remove the value
        casper::remove(keyspace).unwrap();
        // No value exists
        assert_eq!(casper::read_into_vec(keyspace), Ok(None));
        // Removing again (aka removing non-existent key) should raise an error
        assert_eq!(casper::remove(keyspace), Err(CommonResult::NotFound));
        // Re-reading already purged value wouldn't be an issue
        assert_eq!(casper::read_into_vec(keyspace), Ok(None));
        // Write a new value under same key
        casper::write(keyspace, &value_2).unwrap();
        // New value exists
        assert_eq!(casper::read_into_vec(keyspace), Ok(Some(value_2.to_vec())));

        // Attempting to remove a definetely non-existent key should be an error
        let keyspace = Keyspace::Context(b"this key definetely does not exists");
        let result = casper::remove(keyspace);
        assert_eq!(result, Err(CommonResult::NotFound));
    }

    log!("👋 Goodbye");
}

#[casper(export)]
pub fn call(flipper_address: Address) {
    let mut seed = Seed::default();
    perform_test(&mut seed, flipper_address);
}

#[casper(export)]
pub fn yet_another_exported_function(arg1: u64, arg2: String) {
    log!("Yet another exported function with args arg1={arg1} arg2={arg2}");
}

#[cfg(test)]
mod tests {
    use casper::native::{dispatch_with, EntryPointKind, Environment, ENTRY_POINTS};
    use casper_contract_sdk::casper::native::{self, dispatch};
    use contracts::harness::{Harness, INITIAL_GREETING};

    use super::*;
    #[test]
    fn can_call_exported_function() {
        super::yet_another_exported_function(1234u64, "Hello, world!".to_string());

        let input_data = casper_contract_sdk::serializers::borsh::to_vec(&(
            4321u64,
            "!world, Hello".to_string(),
        ))
        .unwrap();

        dispatch_with(Environment::default().with_input_data(input_data), || {
            native::invoke_export_by_name("yet_another_exported_function");
        })
        .unwrap();
    }

    #[test]
    fn exports() {
        assert!(ENTRY_POINTS
            .iter()
            .any(|export| export.kind == EntryPointKind::Function { name: "call" }));
    }

    #[test]
    fn should_greet() {
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
    fn foo() {
        assert_eq!(Harness::default().into_greeting(), "Default value");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    panic!("Execute \"cargo test\" to test the contract, \"cargo build\" to build it");
}
