#![allow(dead_code, unused_variables, non_camel_case_types)]use borsh::{self, BorshSerialize, BorshDeserialize};
use casper_sdk_codegen::support::{IntoResult, IntoOption};
use casper_sdk::Selector;

pub type U8 = u8;
type FixedSequence0_32_U8 = [U8; 32];
/// Declared as ([U8; 32], [U8; 32])
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct __U8__32____U8__32__(FixedSequence0_32_U8, FixedSequence0_32_U8);

pub type Bool = bool;
pub type U64 = u64;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct Map___U8__32____U8__32____U64_ {
    /// Declared as U64
    prefix: U64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct Map__U8__32___U64_ {
    /// Declared as U64
    prefix: U64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct Map__U8__32___vm2_cep18__security_badge__SecurityBadge_ {
    /// Declared as U64
    prefix: U64,
}

/// Declared as vm2_cep18::error::Cep18Error
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
pub enum vm2_cep18__error__Cep18Error {
    InvalidContext(()),
    InsufficientBalance(()),
    InsufficientAllowance(()),
    Overflow(()),
    PackageHashMissing(()),
    PackageHashNotPackage(()),
    InvalidEventsMode(()),
    MissingEventsMode(()),
    Phantom(()),
    FailedToGetArgBytes(()),
    InsufficientRights(()),
    InvalidAdminList(()),
    InvalidMinterList(()),
    InvalidNoneList(()),
    InvalidEnableMBFlag(()),
    AlreadyInitialized(()),
    MintBurnDisabled(()),
    CannotTargetSelfUser(()),
    InvalidBurnTarget(()),
}

/// Declared as Result<(), vm2_cep18::error::Cep18Error>
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
pub enum Result_____vm2_cep18__error__Cep18Error_ {
    Err(vm2_cep18__error__Cep18Error),
    Ok(()),
}

impl IntoResult<(), vm2_cep18__error__Cep18Error> for Result_____vm2_cep18__error__Cep18Error_ {
    fn into_result(self) -> Result<(), vm2_cep18__error__Cep18Error> {
        match self {
        Result_____vm2_cep18__error__Cep18Error_::Ok(ok) => Ok(ok),
        Result_____vm2_cep18__error__Cep18Error_::Err(err) => Err(err),
        }
    }
}

pub type Char = char;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct vm2_cep18__CEP18 {
    /// Declared as String
    name: String,
    /// Declared as String
    symbol: String,
    /// Declared as U8
    decimals: U8,
    /// Declared as U64
    total_supply: U64,
    /// Declared as Map<[U8; 32], U64>
    balances: Map__U8__32___U64_,
    /// Declared as Map<([U8; 32], [U8; 32]), U64>
    allowances: Map___U8__32____U8__32____U64_,
    /// Declared as Map<[U8; 32], vm2_cep18::security_badge::SecurityBadge>
    security_badges: Map__U8__32___vm2_cep18__security_badge__SecurityBadge_,
    /// Declared as Bool
    enable_mint_burn: Bool,
}

/// Declared as vm2_cep18::security_badge::SecurityBadge
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
pub enum vm2_cep18__security_badge__SecurityBadge {
    Admin(()),
    Minter(()),
    None(()),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
pub struct CEP18Client {
    pub address: [u8; 32],
}

impl CEP18Client {
    pub fn new<C>(token_name: String) -> Result<CEP18Client, casper_sdk::types::CallError>
    where C: casper_sdk::Contract,
    {
        const SELECTOR: Selector = Selector::new(2611912030);
        let value = 0; // TODO: Transferring values
        let input_args = (token_name,);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        let create_result = C::create(SELECTOR, Some(&input_data))?;
        let result = CEP18Client { address: create_result.contract_address };
        Ok(result)
    }

    pub fn name(&self) -> Result<casper_sdk::host::CallResult<String>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(987428621);
        let value = 0; // TODO: Transferring values
        let input_args = ();
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn symbol(&self) -> Result<casper_sdk::host::CallResult<String>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(2614203198);
        let value = 0; // TODO: Transferring values
        let input_args = ();
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn decimals(&self) -> Result<casper_sdk::host::CallResult<U8>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(2176884103);
        let value = 0; // TODO: Transferring values
        let input_args = ();
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn total_supply(&self) -> Result<casper_sdk::host::CallResult<U64>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(3680728488);
        let value = 0; // TODO: Transferring values
        let input_args = ();
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn balance_of(&self, address: FixedSequence0_32_U8) -> Result<casper_sdk::host::CallResult<U64>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(259349078);
        let value = 0; // TODO: Transferring values
        let input_args = (address,);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn allowance(&self, spender: FixedSequence0_32_U8, owner: FixedSequence0_32_U8) -> Result<casper_sdk::host::CallResult<()>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(1778390622);
        let value = 0; // TODO: Transferring values
        let input_args = (spender, owner);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn approve(&self, spender: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(1746036384);
        let value = 0; // TODO: Transferring values
        let input_args = (spender, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn decrease_allowance(&self, spender: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(4187548633);
        let value = 0; // TODO: Transferring values
        let input_args = (spender, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn increase_allowance(&self, spender: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(4115780642);
        let value = 0; // TODO: Transferring values
        let input_args = (spender, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn transfer(&self, recipient: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(2225167777);
        let value = 0; // TODO: Transferring values
        let input_args = (recipient, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn transfer_from(&self, owner: FixedSequence0_32_U8, recipient: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(188313368);
        let value = 0; // TODO: Transferring values
        let input_args = (owner, recipient, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn mint(&self, owner: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(3487406754);
        let value = 0; // TODO: Transferring values
        let input_args = (owner, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }

    pub fn burn(&self, owner: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(2985279867);
        let value = 0; // TODO: Transferring values
        let input_args = (owner, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, SELECTOR, &input_args)
    }
}fn main() {}