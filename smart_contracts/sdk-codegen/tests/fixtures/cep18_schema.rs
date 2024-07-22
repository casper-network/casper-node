#![allow(dead_code, unused_variables, non_camel_case_types)]use borsh::{self, BorshSerialize, BorshDeserialize};
use casper_sdk_codegen::support::{IntoResult, IntoOption};
use casper_sdk::{Selector, ToCallData};

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
struct vm2_cep18__traits__CEP18State {
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct vm2_cep18__contract__TokenContract {
    /// Declared as vm2_cep18::traits::CEP18State
    state: vm2_cep18__traits__CEP18State,
}

/// Declared as vm2_cep18::security_badge::SecurityBadge
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
pub enum vm2_cep18__security_badge__SecurityBadge {
    Admin(()),
    Minter(()),
    None(()),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
pub struct TokenContractClient {
    pub address: [u8; 32],
}

impl TokenContractClient {
    pub fn new<C>(token_name: String) -> Result<TokenContractClient, casper_sdk::types::CallError>
    where C: casper_sdk::Contract,
    {
        const SELECTOR: Selector = Selector::new(2611912030);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_new { 
        token_name,
        };
        let create_result = C::create(call_data)?;
        let result = TokenContractClient { address: create_result.contract_address };
        Ok(result)
    }

    pub fn my_balance(&self) -> Result<casper_sdk::host::CallResult<U64>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(926069361);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_my_balance;
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn name(&self) -> Result<casper_sdk::host::CallResult<String>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(987428621);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_name;
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn symbol(&self) -> Result<casper_sdk::host::CallResult<String>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(2614203198);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_symbol;
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn decimals(&self) -> Result<casper_sdk::host::CallResult<U8>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(2176884103);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_decimals;
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn total_supply(&self) -> Result<casper_sdk::host::CallResult<U64>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(3680728488);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_total_supply;
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn balance_of(&self, address: FixedSequence0_32_U8) -> Result<casper_sdk::host::CallResult<U64>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(259349078);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_balance_of { 
        address,
        };
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn allowance(&self, spender: FixedSequence0_32_U8, owner: FixedSequence0_32_U8) -> Result<casper_sdk::host::CallResult<()>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(1778390622);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_allowance { 
        spender,
        owner,
        };
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn approve(&self, spender: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(1746036384);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_approve { 
        spender,
        amount,
        };
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn decrease_allowance(&self, spender: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(4187548633);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_decrease_allowance { 
        spender,
        amount,
        };
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn increase_allowance(&self, spender: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(4115780642);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_increase_allowance { 
        spender,
        amount,
        };
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn transfer(&self, recipient: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(2225167777);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_transfer { 
        recipient,
        amount,
        };
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn transfer_from(&self, owner: FixedSequence0_32_U8, recipient: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(188313368);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_transfer_from { 
        owner,
        recipient,
        amount,
        };
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn mint(&self, owner: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(3487406754);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_mint { 
        owner,
        amount,
        };
        casper_sdk::host::call(&self.address, value, call_data)
    }

    pub fn burn(&self, owner: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Result_____vm2_cep18__error__Cep18Error_>, casper_sdk::types::CallError> {
        const SELECTOR: Selector = Selector::new(2985279867);
        let value = 0; // TODO: Transferring values
        let call_data = TokenContract_burn { 
        owner,
        amount,
        };
        casper_sdk::host::call(&self.address, value, call_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_new {
    token_name: String,
}

impl ToCallData for TokenContract_new {
     const SELECTOR: Selector = Selector::new(2611912030);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_my_balance;

impl ToCallData for TokenContract_my_balance {
     const SELECTOR: Selector = Selector::new(926069361);
    fn input_data(&self) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_name;

impl ToCallData for TokenContract_name {
     const SELECTOR: Selector = Selector::new(987428621);
    fn input_data(&self) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_symbol;

impl ToCallData for TokenContract_symbol {
     const SELECTOR: Selector = Selector::new(2614203198);
    fn input_data(&self) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_decimals;

impl ToCallData for TokenContract_decimals {
     const SELECTOR: Selector = Selector::new(2176884103);
    fn input_data(&self) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_total_supply;

impl ToCallData for TokenContract_total_supply {
     const SELECTOR: Selector = Selector::new(3680728488);
    fn input_data(&self) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_balance_of {
    address: FixedSequence0_32_U8,
}

impl ToCallData for TokenContract_balance_of {
     const SELECTOR: Selector = Selector::new(259349078);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_allowance {
    spender: FixedSequence0_32_U8,
    owner: FixedSequence0_32_U8,
}

impl ToCallData for TokenContract_allowance {
     const SELECTOR: Selector = Selector::new(1778390622);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_approve {
    spender: FixedSequence0_32_U8,
    amount: U64,
}

impl ToCallData for TokenContract_approve {
     const SELECTOR: Selector = Selector::new(1746036384);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_decrease_allowance {
    spender: FixedSequence0_32_U8,
    amount: U64,
}

impl ToCallData for TokenContract_decrease_allowance {
     const SELECTOR: Selector = Selector::new(4187548633);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_increase_allowance {
    spender: FixedSequence0_32_U8,
    amount: U64,
}

impl ToCallData for TokenContract_increase_allowance {
     const SELECTOR: Selector = Selector::new(4115780642);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_transfer {
    recipient: FixedSequence0_32_U8,
    amount: U64,
}

impl ToCallData for TokenContract_transfer {
     const SELECTOR: Selector = Selector::new(2225167777);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_transfer_from {
    owner: FixedSequence0_32_U8,
    recipient: FixedSequence0_32_U8,
    amount: U64,
}

impl ToCallData for TokenContract_transfer_from {
     const SELECTOR: Selector = Selector::new(188313368);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_mint {
    owner: FixedSequence0_32_U8,
    amount: U64,
}

impl ToCallData for TokenContract_mint {
     const SELECTOR: Selector = Selector::new(3487406754);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
struct TokenContract_burn {
    owner: FixedSequence0_32_U8,
    amount: U64,
}

impl ToCallData for TokenContract_burn {
     const SELECTOR: Selector = Selector::new(2985279867);
    fn input_data(&self) -> Option<Vec<u8>> {
        let input_data = borsh::to_vec(&self).expect("Serialization to succeed");
        Some(input_data)
    }
}fn main() {}