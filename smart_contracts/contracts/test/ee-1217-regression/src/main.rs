#![no_std]
#![no_main]

extern crate alloc;
use alloc::{string::ToString, vec};

// casper_contract is required for it's [global_alloc] as well as handlers (such as panic_handler)
use casper_contract::contract_api::{runtime, storage, system};
use casper_types::{
    runtime_args, system::auction, CLType, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, PublicKey, RuntimeArgs, U512,
};

const PACKAGE_NAME: &str = "call_auction";
const PACKAGE_ACCESS_KEY_NAME: &str = "call_auction_access";

const METHOD_CALL_AUCTION_NAME: &str = "call_auction";

#[no_mangle]
pub extern "C" fn call_auction() {
    let public_key: PublicKey = runtime::get_named_arg(auction::ARG_PUBLIC_KEY);
    let auction = system::get_auction();
    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_AMOUNT => U512::one(),
        auction::ARG_DELEGATION_RATE => 42u8,
    };
    runtime::call_contract::<U512>(auction, auction::METHOD_ADD_BID, args);
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            METHOD_CALL_AUCTION_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        entry_points
    };

    let (_contract_hash, _contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_NAME.to_string()),
        Some(PACKAGE_ACCESS_KEY_NAME.to_string()),
    );
}
