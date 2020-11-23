#![no_std]

#[macro_use]
extern crate alloc;

use alloc::boxed::Box;

use casper_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    contracts::Parameters,
    mint::{
        Mint, RuntimeProvider, StorageProvider, SystemProvider, ARG_AMOUNT, ARG_ID, ARG_PURSE,
        ARG_SOURCE, ARG_TARGET, METHOD_BALANCE, METHOD_CREATE, METHOD_MINT,
        METHOD_READ_BASE_ROUND_REWARD, METHOD_TRANSFER,
    },
    system_contract_errors::mint::Error,
    CLType, CLTyped, CLValue, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    Parameter, URef, U512,
};

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

impl SystemProvider for MintContract {
    fn record_transfer(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<(), Error> {
        system::record_transfer(source, target, amount, id)
            .map_err(|_| Error::RecordTransferFailure)
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
    let id: Option<u64> = runtime::get_named_arg(ARG_ID);
    let result: Result<(), Error> = mint_contract.transfer(source, target, amount, id);
    let ret = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(ret);
}

pub fn read_base_round_reward() {
    let mut mint_contract = MintContract;
    let result: Result<U512, Error> = mint_contract.read_base_round_reward();
    let ret = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(ret);
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
            Parameter::new(ARG_ID, CLType::Option(Box::new(CLType::U64))),
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
        METHOD_READ_BASE_ROUND_REWARD,
        Parameters::new(),
        CLType::U512,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    entry_points
}
