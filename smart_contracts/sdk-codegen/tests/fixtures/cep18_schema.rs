#![allow(dead_code, unused_variables, non_camel_case_types)]use borsh::{self, BorshSerialize, BorshDeserialize};

pub type U8 = u8;
type FixedSequence0_32_U8 = [U8; 32];
/// Declared as ([U8; 32], [U8; 32])
#[derive(BorshSerialize, BorshDeserialize)]
struct Tuple1(FixedSequence0_32_U8, FixedSequence0_32_U8);

pub type Bool = bool;
pub type U64 = u64;
#[derive(BorshSerialize, BorshDeserialize)]
struct Struct2 {
    /// Declared as U64
    prefix: U64,
}

#[derive(BorshSerialize, BorshDeserialize)]
struct Struct3 {
    /// Declared as U64
    prefix: U64,
}

#[derive(BorshSerialize, BorshDeserialize)]
struct Struct4 {
    /// Declared as U64
    prefix: U64,
}

/// Declared as vm2_cep18::error::Cep18Error
#[derive(BorshSerialize, BorshDeserialize)]
pub enum Enum5 {
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
#[derive(BorshSerialize, BorshDeserialize)]
pub enum Enum6 {
    Ok(()),
    Err(Enum5),
}

pub type Char = char;
#[derive(BorshSerialize, BorshDeserialize)]
struct Struct7 {
    /// Declared as String
    name: String,
    /// Declared as String
    symbol: String,
    /// Declared as U8
    decimals: U8,
    /// Declared as U64
    total_supply: U64,
    /// Declared as Map<[U8; 32], U64>
    balances: Struct3,
    /// Declared as Map<([U8; 32], [U8; 32]), U64>
    allowances: Struct2,
    /// Declared as Map<[U8; 32], vm2_cep18::security_badge::SecurityBadge>
    security_badges: Struct4,
    /// Declared as Bool
    enable_mint_burn: Bool,
}

/// Declared as vm2_cep18::security_badge::SecurityBadge
#[derive(BorshSerialize, BorshDeserialize)]
pub enum Enum8 {
    Admin(()),
    Minter(()),
    None(()),
}

pub struct CEP18Client {
    pub address: [u8; 32],
}

impl CEP18Client {
    pub fn new<C>() -> Result<CEP18Client, casper_sdk::types::CallError>
    where C: casper_sdk::Contract,
    {
        let value = 0; // TODO: Transferring values
        let input_args = ();
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        let create_result = C::create(Some("new"), None)?;
        let result = CEP18Client { address: create_result.contract_address };
        Ok(result)
    }

    pub fn name(&self) -> Result<casper_sdk::host::CallResult<String>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = ();
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "name", &input_data)
    }

    pub fn symbol(&self) -> Result<casper_sdk::host::CallResult<String>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = ();
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "symbol", &input_data)
    }

    pub fn decimals(&self) -> Result<casper_sdk::host::CallResult<U8>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = ();
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "decimals", &input_data)
    }

    pub fn total_supply(&self) -> Result<casper_sdk::host::CallResult<U64>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = ();
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "total_supply", &input_data)
    }

    pub fn balance_of(&self, address: FixedSequence0_32_U8) -> Result<casper_sdk::host::CallResult<U64>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = (address,);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "balance_of", &input_data)
    }

    pub fn allowance(&self, spender: FixedSequence0_32_U8, owner: FixedSequence0_32_U8) -> Result<casper_sdk::host::CallResult<()>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = (spender, owner);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "allowance", &input_data)
    }

    pub fn approve(&self, spender: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Enum6>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = (spender, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "approve", &input_data)
    }

    pub fn decrease_allowance(&self, spender: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Enum6>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = (spender, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "decrease_allowance", &input_data)
    }

    pub fn increase_allowance(&self, spender: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Enum6>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = (spender, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "increase_allowance", &input_data)
    }

    pub fn transfer(&self, recipient: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Enum6>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = (recipient, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "transfer", &input_data)
    }

    pub fn transfer_from(&self, owner: FixedSequence0_32_U8, recipient: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Enum6>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = (owner, recipient, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "transfer_from", &input_data)
    }

    pub fn mint(&self, owner: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Enum6>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = (owner, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "mint", &input_data)
    }

    pub fn burn(&self, owner: FixedSequence0_32_U8, amount: U64) -> Result<casper_sdk::host::CallResult<Enum6>, casper_sdk::types::CallError> {
        let value = 0; // TODO: Transferring values
        let input_args = (owner, amount);
        let input_data = borsh::to_vec(&input_args).expect("Serialization to succeed");
        casper_sdk::host::call(&self.address, value, "burn", &input_data)
    }
}fn main() {}