use std::collections::{BTreeSet, HashMap, LinkedList};

use casper_contract_macros::casper;
use casper_contract_sdk::{
    casper, collections::Map, log, prelude::Entity, revert, types::CallError, ContractHandle,
};

use crate::traits::{DepositExt, DepositRef};

pub const INITIAL_GREETING: &str = "This is initial data set from a constructor";
pub(crate) const BALANCES_PREFIX: &str = "b";

#[derive(Debug)]
#[casper(contract_state)]
pub struct Harness {
    counter: u64,
    greeting: String,
    address_inside_constructor: Option<Entity>,
    balances: Map<Entity, u64>,
    block_time: u64,
}

// #[casper(path = crate::traits)]
// impl Fallback for Harness {
//     fn fallback(&mut self) {
//         // Called when no entrypoint is matched
//         //
//         // Is invoked when
//         // a) user performs plan token transfer (not a contract call)
//         //   a.1) if there's no fallback entrypoint, the transfer will fail
//         //   a.2) if there's fallback entrypoint, it will be called
//         // b) user calls a contract with no matching entrypoint
//         //   b.1) if there's no fallback entrypoint, the call will fail
//         //   b.2) if there's fallback entrypoint, it will be called and user can

//         log!(
//             "Harness received fallback entrypoint value={}",
//             host::get_value()
//         );
//     }
// }

#[derive(Debug, thiserror::Error, PartialEq)]
#[casper]
pub enum CustomError {
    #[error("foo")]
    Foo,
    #[error("bar")]
    Bar = 42,
    #[error("error with body {0}")]
    WithBody(String),
    #[error("error with named variant name={name}; age={age}")]
    Named { name: String, age: u64 },
    #[error("transfer error {0}")]
    Transfer(String),
    #[error("deposit error {0}")]
    Deposit(CallError),
}

#[derive(Debug, PartialEq)]
#[casper]
pub struct PublicStructUsedOnlyByPrivateEntrypoint {
    pub value: u64,
}

impl Default for Harness {
    fn default() -> Self {
        Self {
            counter: 0,
            greeting: "Default value".to_string(),
            address_inside_constructor: None,
            balances: Map::new(BALANCES_PREFIX),
            block_time: 0,
        }
    }
}

pub type Result2 = Result<(), CustomError>;

#[casper]
impl Harness {
    // #[casper(event)]
    // type TestMessage;

    #[casper(constructor)]
    pub fn constructor_with_args(who: String) -> Self {
        // Event::register();

        log!("👋 Hello from constructor with args: {who}");

        Self {
            counter: 0,
            greeting: format!("Hello, {who}!"),
            address_inside_constructor: Some(casper::get_caller()),
            balances: Map::new(BALANCES_PREFIX),
            block_time: casper::get_block_time(),
        }
    }

    #[casper(constructor)]
    pub fn failing_constructor(who: String) -> Self {
        log!("👋 Hello from failing constructor with args: {who}");
        revert!();
    }

    #[casper(constructor)]
    pub fn trapping_constructor() -> Self {
        log!("👋 Hello from trapping constructor");
        panic!("This will revert the execution of this constructor and won't create a new package");
    }

    #[casper(constructor)]
    pub fn initialize() -> Self {
        log!("👋 Hello from constructor");
        Self {
            counter: 0,
            greeting: INITIAL_GREETING.to_string(),
            address_inside_constructor: Some(casper::get_caller()),
            balances: Map::new(BALANCES_PREFIX),
            block_time: casper::get_block_time(),
        }
    }

    #[casper(constructor, payable)]
    pub fn payable_constructor() -> Self {
        log!(
            "👋 Hello from payable constructor value={}",
            casper::transferred_value()
        );
        Self {
            counter: 0,
            greeting: INITIAL_GREETING.to_string(),
            address_inside_constructor: Some(casper::get_caller()),
            balances: Map::new(BALANCES_PREFIX),
            block_time: casper::get_block_time(),
        }
    }

    #[casper(constructor, payable)]
    pub fn payable_failing_constructor() -> Self {
        log!(
            "👋 Hello from payable failign constructor value={}",
            casper::transferred_value()
        );
        revert!();
    }

    #[casper(constructor, payable)]
    pub fn payable_trapping_constructor() -> Self {
        log!(
            "👋 Hello from payable trapping constructor value={}",
            casper::transferred_value()
        );
        panic!("This will revert the execution of this constructor and won't create a new package")
    }

    pub fn get_greeting(&self) -> &str {
        &self.greeting
    }

    pub fn increment_counter(&mut self) {
        self.counter += 1;
    }

    pub fn counter(&self) -> u64 {
        self.counter
    }

    pub fn set_greeting(&mut self, greeting: String) {
        self.counter += 1;
        log!("Saving greeting {}", greeting);
        self.greeting = greeting;
    }

    pub fn emit_unreachable_trap(&mut self) -> ! {
        self.counter += 1;
        panic!("unreachable");
    }

    #[casper(revert_on_error)]
    pub fn emit_revert_with_data(&mut self) -> Result<(), CustomError> {
        // revert(code), ret(bytes)

        // casper_return(flags, bytes) flags == 0, flags & FLAG_REVERT
        log!("emit_revert_with_data state={:?}", self);
        log!(
            "Reverting with data before {counter}",
            counter = self.counter
        );
        self.counter += 1;
        log!(
            "Reverting with data after {counter}",
            counter = self.counter
        );
        // Here we can't use revert!() macro, as it explicitly calls `return` and does not involve
        // writing the state again.
        Err(CustomError::Bar)
    }

    pub fn emit_revert_without_data(&mut self) -> ! {
        self.counter += 1;
        revert!()
    }

    pub fn get_address_inside_constructor(&self) -> Entity {
        self.address_inside_constructor
            .expect("Constructor was expected to be caller")
    }

    #[casper(revert_on_error)]
    pub fn should_revert_on_error(&self, flag: bool) -> Result2 {
        if flag {
            Err(CustomError::WithBody("Reverted".into()))
        } else {
            Ok(())
        }
    }

    #[allow(dead_code)]
    fn private_function_that_should_not_be_exported(&self) {
        log!("This function should not be callable from outside");
    }

    pub(crate) fn restricted_function_that_should_be_part_of_manifest(&self) {
        log!("This function should be callable from outside");
    }

    pub fn entry_point_without_state() {
        log!("This function does not require state");
    }

    pub fn entry_point_without_state_with_args_and_output(mut arg: String) -> String {
        log!("This function does not require state");
        arg.push_str("extra");
        arg
    }

    pub fn into_modified_greeting(mut self) -> String {
        self.greeting.push_str("!");
        self.greeting
    }

    pub fn into_greeting(self) -> String {
        self.greeting
    }

    #[casper(payable)]
    pub fn payable_entrypoint(&mut self) -> Result<(), CustomError> {
        log!(
            "This is a payable entrypoint value={}",
            casper::transferred_value()
        );
        Ok(())
    }

    // enum Error {
    //     TooLow { expected: u64}
    // }

    // // #[casper(payable)]
    // pub fn mint_wrapped_token(&mut self) -> Result<(), Error> {

    //     if host::get_transferred_value() < EXPECTED_AMOUNT {
    //         // abort!("This function is not payable");
    //         return Err(Error::TooLow { expected: EXPECTED_AMOUNT });
    //         // abort!("This function is not payable");
    //         // panic_str
    //         // abort!("")
    //     }

    //     let transferred_value = host::get_transferred_value();
    //     self.balances[sender] += transferred_value;
    // }

    #[casper(payable, revert_on_error)]
    pub fn payable_failing_entrypoint(&self) -> Result<(), CustomError> {
        log!(
            "This is a payable entrypoint with value={}",
            casper::transferred_value()
        );
        if casper::transferred_value() == 123 {
            Err(CustomError::Foo)
        } else {
            Ok(())
        }
    }

    #[casper(payable, revert_on_error)]
    pub fn perform_token_deposit(&mut self, balance_before: u64) -> Result<(), CustomError> {
        let caller = casper::get_caller();
        let value = casper::transferred_value();

        if dbg!(value) == 0 {
            return Err(CustomError::WithBody(
                "Value should be greater than 0".into(),
            ));
        }

        assert_eq!(
            balance_before
                .checked_sub(value)
                .unwrap_or_else(|| panic!("Balance before should be larger or equal to the value (caller={caller:?}, value={value})")),
            casper::get_balance_of(&caller),
            "Balance mismatch; token transfer should happen before a contract call"
        );

        log!("Depositing {value} from {caller:?}");
        let current_balance = self.balances.get(&caller).unwrap_or(0);
        self.balances.insert(&caller, &(current_balance + value));
        Ok(())
    }

    #[casper(revert_on_error)]
    pub fn withdraw(&mut self, balance_before: u64, amount: u64) -> Result<(), CustomError> {
        let caller = casper::get_caller();
        log!("Withdrawing {amount} into {caller:?}");
        let current_balance = self.balances.get(&caller).unwrap_or(0);
        if current_balance < amount {
            return Err(CustomError::WithBody("Insufficient balance".into()));
        }

        match caller {
            Entity::Account(account) => {
                log!("caller account {:?}", account);
                // if this fails, the transfer will be reverted and the state will be rolled back
                match casper::transfer(&caller.entity_addr(), amount) {
                    Ok(_) => {}
                    Err(call_error) => {
                        log!("Unable to perform a transfer: {call_error:?}");
                        return Err(CustomError::Transfer(call_error.to_string()));
                    }
                }
            }
            Entity::Contract(contract) => {
                log!("caller contract {:?}", contract);
                let result = ContractHandle::<DepositRef>::from_address(contract)
                    .build_call()
                    .with_transferred_value(amount)
                    .try_call(|harness| harness.deposit());

                match result {
                    Ok(call_result) => {
                        if let Err(call_error) = call_result.result {
                            log!("CallResult: Unable to perform a transfer: {call_error:?}");
                            return Err(CustomError::Deposit(call_error));
                        }
                    }
                    Err(call_error) => {
                        log!("try_call: Unable to perform a transfer: {call_error:?}");
                        return Err(CustomError::Deposit(call_error));
                    }
                }
            }
        }

        let balance_after = balance_before + amount;

        log!("balance_after {}", balance_after);
        assert_eq!(
            casper::get_balance_of(&caller),
            balance_after,
            "Balance should be updated after withdrawal"
        );

        self.balances.insert(&caller, &(current_balance - amount));
        Ok(())
    }

    pub fn balance(&self) -> u64 {
        if casper::transferred_value() != 0 {
            panic!("This function is not payable");
        }
        let caller = casper::get_caller();
        self.balances.get(&caller).unwrap_or(0)
    }

    pub fn new_method(
        &self,
        _arg1: i32,
        _arg2: i64,
        _arg3: u32,
        _arg4: u64,
        _arg5: u64,
        _arg6: Vec<u64>,
        _arg7: bool,
        _arg8: i8,
        _arg9: String,
        _arg10: Vec<u8>,
        _arg11: [u32; 5],
        _arg12: Option<String>,
        _arg13: Result<(), ()>,
        _arg14: Box<i32>,
        _arg15: String,
        _arg16: i32,
        _arg17: u64,
        _arg18: (i32, i32),
        _arg19: HashMap<String, i32>,
        _arg20: BTreeSet<i32>,
        _arg21: LinkedList<String>,
        _arg22: String,
        _arg23: u64,
    ) {
        log!("Nothing");
    }

    #[casper(private)]
    pub fn private_only_uses_public_struct(&self, _arg: PublicStructUsedOnlyByPrivateEntrypoint) {
        log!("This function should be private and its arg type should not appear in schema");
    }
}
