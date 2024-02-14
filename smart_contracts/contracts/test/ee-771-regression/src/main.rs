#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::ToString;

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    addressable_entity::{NamedKeys, Parameters},
    AddressableEntityHash, CLType, EntityVersion, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Key, RuntimeArgs,
};

const ENTRY_POINT_NAME: &str = "contract_ext";
const CONTRACT_KEY: &str = "contract";

#[no_mangle]
pub extern "C" fn contract_ext() {
    match runtime::get_key(CONTRACT_KEY) {
        Some(contract_key) => {
            // Calls a stored contract if exists.
            runtime::call_contract(
                contract_key
                    .into_entity_hash_addr()
                    .expect("should be a hash")
                    .into(),
                "contract_ext",
                RuntimeArgs::default(),
            )
        }
        None => {
            // If given key doesn't exist it's the tail call, and an error is triggered.
            let entry_points = {
                let mut entry_points = EntryPoints::new();

                let entry_point = EntryPoint::new(
                    "functiondoesnotexist",
                    Parameters::default(),
                    CLType::Unit,
                    EntryPointAccess::Public,
                    EntryPointType::AddressableEntity,
                );

                entry_points.add_entry_point(entry_point);

                entry_points
            };
            storage::new_contract(entry_points, None, None, None);
        }
    }
}

fn store(named_keys: NamedKeys) -> (AddressableEntityHash, EntityVersion) {
    // extern "C" fn call(named_keys: NamedKeys) {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            ENTRY_POINT_NAME,
            Parameters::default(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );

        entry_points.add_entry_point(entry_point);

        entry_points
    };
    storage::new_contract(entry_points, Some(named_keys), None, None)
}

fn install() -> AddressableEntityHash {
    let (contract_hash, _contract_version) = store(NamedKeys::new());

    let mut keys = NamedKeys::new();
    keys.insert(
        CONTRACT_KEY.to_string(),
        Key::contract_entity_key(contract_hash),
    );
    let (contract_hash, _contract_version) = store(keys);

    let mut keys_2 = NamedKeys::new();
    keys_2.insert(
        CONTRACT_KEY.to_string(),
        Key::contract_entity_key(contract_hash),
    );
    let (contract_hash, _contract_version) = store(keys_2);

    runtime::put_key(CONTRACT_KEY, Key::contract_entity_key(contract_hash));

    contract_hash
}

fn dispatch(contract_hash: AddressableEntityHash) {
    runtime::call_contract(contract_hash, "contract_ext", RuntimeArgs::default())
}

#[no_mangle]
pub extern "C" fn call() {
    let contract_key = install();
    dispatch(contract_key)
}
