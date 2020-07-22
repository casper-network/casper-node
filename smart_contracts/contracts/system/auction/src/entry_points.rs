use alloc::{boxed::Box, vec::Vec};
use core::result::Result as StdResult;

use contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    runtime_args, CLType, CLTyped, CLValue, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Key, Parameter, RuntimeArgs, URef, U512,
};

use crate::{
    providers::{ProofOfStakeProvider, StorageProvider, SystemProvider},
    Auction, DelegationRate,
};

const ARG_ACCOUNT_HASH: &str = "account_hash";

const ARG_AMOUNT: &str = "amount";
const ARG_PURSE: &str = "purse";
const BOND: &str = "bond";

struct AuctionContract;

impl StorageProvider for AuctionContract {
    type Error = crate::Error;
    fn get_key(&mut self, name: &str) -> Option<Key> {
        runtime::get_key(name)
    }

    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Self::Error> {
        Ok(storage::read(uref)?)
    }
    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) {
        storage::write(uref, value);
    }
}

impl ProofOfStakeProvider for AuctionContract {
    fn bond(&mut self, amount: U512, purse: URef) {
        let contract_hash = system::get_proof_of_stake();
        let args = runtime_args! {
            ARG_AMOUNT => amount,
            ARG_PURSE => purse,
        };
        runtime::call_contract::<()>(contract_hash, BOND, args);
    }
}

impl SystemProvider for AuctionContract {
    type Error = crate::Error;
    fn create_purse(&mut self) -> URef {
        system::create_purse()
    }
    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> StdResult<(), Self::Error> {
        system::transfer_from_purse_to_purse(source, target, amount)
            .map_err(|_| crate::Error::Transfer)
    }
}

impl Auction for AuctionContract {}

#[no_mangle]
pub extern "C" fn release_founder() {
    let account_hash = runtime::get_named_arg("account_hash");

    let result = AuctionContract.release_founder(account_hash);

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value);
}

#[no_mangle]
pub extern "C" fn read_winners() {
    let result = AuctionContract.read_winners();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn read_seigniorage_recipients() {
    let result = AuctionContract.read_seigniorage_recipients();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn add_bid() {
    let account_hash = runtime::get_named_arg("account_hash");
    let source_purse = runtime::get_named_arg("source_purse");
    let delegation_rate = runtime::get_named_arg("delegation_rate");
    let quantity = runtime::get_named_arg("quantity");

    let result = AuctionContract
        .add_bid(account_hash, source_purse, delegation_rate, quantity)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn delegate() {
    let delegator_account_hash = runtime::get_named_arg("delegator_account_hash");
    let source_purse = runtime::get_named_arg("source_purse");
    let validator_account_hash = runtime::get_named_arg("validator_account_hash");
    let quantity = runtime::get_named_arg("quantity");

    let result = AuctionContract
        .delegate(
            delegator_account_hash,
            source_purse,
            validator_account_hash,
            quantity,
        )
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn undelegate() {
    let delegator_account_hash = runtime::get_named_arg("delegator_account_hash");
    let validator_account_hash = runtime::get_named_arg("validator_account_hash");
    let quantity = runtime::get_named_arg("quantity");

    let result = AuctionContract
        .undelegate(delegator_account_hash, validator_account_hash, quantity)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn squash_bid() {
    let validator_keys: Vec<AccountHash> = runtime::get_named_arg("validator_keys");

    AuctionContract.squash_bid(&validator_keys);
}

#[no_mangle]
pub extern "C" fn run_auction() {
    AuctionContract.run_auction();
}

pub fn get_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let entry_point = EntryPoint::new(
        "release_founder",
        vec![Parameter::new(ARG_ACCOUNT_HASH, AccountHash::cl_type())],
        CLType::Bool,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        "read_winners",
        vec![],
        CLType::List(Box::new(AccountHash::cl_type())),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        "read_seigniorage_recipients",
        vec![],
        CLType::Map {
            key: Box::new(AccountHash::cl_type()),
            value: Box::new(CLType::Any),
        },
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        "add_bid",
        vec![
            Parameter::new("account_hash", AccountHash::cl_type()),
            Parameter::new("source_purse", URef::cl_type()),
            Parameter::new("delegation_rate", DelegationRate::cl_type()),
            Parameter::new("quantity", U512::cl_type()),
        ],
        CLType::Result {
            ok: Box::new(CLType::Tuple2([
                Box::new(URef::cl_type()),
                Box::new(U512::cl_type()),
            ])),
            err: Box::new(u32::cl_type()),
        },
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        "withdraw_bid",
        vec![
            Parameter::new("account_hash", AccountHash::cl_type()),
            Parameter::new("quantity", U512::cl_type()),
        ],
        crate::Result::<(URef, U512)>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        "delegate",
        vec![
            Parameter::new("delegator_account_hash", AccountHash::cl_type()),
            Parameter::new("source_purse", URef::cl_type()),
            Parameter::new("validator_account_hash", AccountHash::cl_type()),
            Parameter::new("quantity", U512::cl_type()),
        ],
        crate::Result::<(URef, U512)>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        "undelegate",
        vec![
            Parameter::new("delegator_account_hash", AccountHash::cl_type()),
            Parameter::new("validator_account_hash", AccountHash::cl_type()),
            Parameter::new("quantity", U512::cl_type()),
        ],
        crate::Result::<(URef, U512)>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        "squash_bid",
        vec![Parameter::new(
            "validator_keys",
            Vec::<AccountHash>::cl_type(),
        )],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        "run_auction",
        vec![],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    entry_points
}
