#![no_std]

#[macro_use]
extern crate alloc;

use alloc::boxed::Box;

use casperlabs_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    account::AccountHash,
    auction::METHOD_READ_ERA_ID,
    bytesrepr::{FromBytes, ToBytes},
    contracts::Parameters,
    mint::{EraProvider, Mint, RuntimeProvider, StorageProvider},
    system_contract_errors::mint::Error,
    CLType, CLTyped, CLValue, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    Parameter, RuntimeArgs, URef, U512,
};

pub const METHOD_MINT: &str = "mint";
pub const METHOD_CREATE: &str = "create";
pub const METHOD_BALANCE: &str = "balance";
pub const METHOD_TRANSFER: &str = "transfer";
pub const METHOD_BOND: &str = "bond";
pub const METHOD_UNBOND: &str = "unbond";
pub const METHOD_UNBOND_TIMER_ADVANCE: &str = "unbond_timer_advance";
pub const METHOD_SLASH: &str = "slash";
pub const METHOD_RELEASE_FOUNDER_STAKE: &str = "release_founder_stake";

pub const ARG_AMOUNT: &str = "amount";
pub const ARG_PURSE: &str = "purse";
pub const ARG_SOURCE: &str = "source";
pub const ARG_TARGET: &str = "target";

pub struct MintContract;

impl RuntimeProvider for MintContract {
    fn get_caller(&self) -> AccountHash {
        runtime::get_caller()
    }

    fn put_key(&mut self, name: &str, key: Key) {
        runtime::put_key(name, key)
    }
    fn get_key(&self, name: &str) -> Option<Key> {
        runtime::get_key(name)
    }
}

impl StorageProvider for MintContract {
    fn new_uref<T: CLTyped + ToBytes>(&mut self, init: T) -> URef {
        storage::new_uref(init)
    }

    fn write_local<K: ToBytes, V: CLTyped + ToBytes>(&mut self, key: K, value: V) {
        storage::write_local(key, value)
    }

    fn read_local<K: ToBytes, V: CLTyped + FromBytes>(
        &mut self,
        key: &K,
    ) -> Result<Option<V>, Error> {
        storage::read_local(key).map_err(|_| Error::Storage)
    }

    fn read<T: CLTyped + FromBytes>(&mut self, uref: URef) -> Result<Option<T>, Error> {
        storage::read(uref).map_err(|_| Error::Storage)
    }

    fn write<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        storage::write(uref, value);
        Ok(())
    }

    fn add<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        storage::add(uref, value);
        Ok(())
    }
}

impl EraProvider for MintContract {
    fn read_era_id(&mut self) -> casperlabs_types::auction::EraId {
        let auction = system::get_auction();
        runtime::call_contract(auction, METHOD_READ_ERA_ID, RuntimeArgs::new())
    }
}

impl Mint for MintContract {}

pub fn mint() {
    let mut mint_contract = MintContract;
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let result: Result<URef, Error> = mint_contract.mint(amount);
    let ret = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(ret)
}

pub fn create() {
    let mut mint_contract = MintContract;
    let uref = mint_contract.mint(U512::zero()).unwrap_or_revert();
    let ret = CLValue::from_t(uref).unwrap_or_revert();
    runtime::ret(ret)
}

pub fn balance() {
    let mut mint_contract = MintContract;
    let uref: URef = runtime::get_named_arg(ARG_PURSE);
    let balance: Option<U512> = mint_contract.balance(uref).unwrap_or_revert();
    let ret = CLValue::from_t(balance).unwrap_or_revert();
    runtime::ret(ret)
}

pub fn transfer() {
    let mut mint_contract = MintContract;
    let source: URef = runtime::get_named_arg(ARG_SOURCE);
    let target: URef = runtime::get_named_arg(ARG_TARGET);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let result: Result<(), Error> = mint_contract.transfer(source, target, amount);
    let ret = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(ret);
}

pub fn bond() {
    let mut mint_contract = MintContract;
    let account_hash = runtime::get_caller();

    let source_purse = runtime::get_named_arg(ARG_PURSE);
    let quantity = runtime::get_named_arg(ARG_AMOUNT);
    let result = mint_contract
        .bond(account_hash, source_purse, quantity)
        .unwrap_or_revert();
    let ret = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(ret);
}

pub fn unbond() {
    let mut mint_contract = MintContract;
    let account_hash = runtime::get_caller();
    let quantity = runtime::get_named_arg(ARG_AMOUNT);
    let result = mint_contract
        .unbond(account_hash, quantity)
        .unwrap_or_revert();
    let ret = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(ret)
}

pub fn unbond_timer_advance() {
    let mut mint_contract = MintContract;
    mint_contract.unbond_timer_advance().unwrap_or_revert();
}

pub fn slash() {
    let mut mint_contract = MintContract;
    let validator_account_hashes = runtime::get_named_arg("validator_account_hashes");
    mint_contract
        .slash(validator_account_hashes)
        .unwrap_or_revert();
}

pub fn get_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let entry_point = EntryPoint::new(
        METHOD_MINT,
        vec![Parameter::new(ARG_AMOUNT, CLType::U512)],
        CLType::Result {
            ok: Box::new(CLType::URef),
            err: Box::new(CLType::U8),
        },
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_CREATE,
        Parameters::new(),
        CLType::URef,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_BALANCE,
        vec![Parameter::new(ARG_PURSE, CLType::URef)],
        CLType::Option(Box::new(CLType::U512)),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_TRANSFER,
        vec![
            Parameter::new(ARG_SOURCE, CLType::URef),
            Parameter::new(ARG_TARGET, CLType::URef),
            Parameter::new(ARG_AMOUNT, CLType::U512),
        ],
        CLType::Result {
            ok: Box::new(CLType::Unit),
            err: Box::new(CLType::U8),
        },
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_BOND,
        vec![
            Parameter::new(ARG_PURSE, CLType::URef),
            Parameter::new(ARG_AMOUNT, CLType::U512),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_UNBOND,
        vec![],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_UNBOND_TIMER_ADVANCE,
        vec![],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_SLASH,
        vec![],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    entry_points
}
