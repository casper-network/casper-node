#![no_std]

extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::NamedKeys, AccessRights, ApiError, CLType, CLValue, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, URef,
};

pub const DICTIONARY_NAME: &str = "local";
pub const DICTIONARY_PUT_KEY: [u8; 32] = [66u8; 32];
pub const HELLO_PREFIX: &str = " Hello, ";
pub const WORLD_SUFFIX: &str = "world!";
pub const MODIFY_WRITE_ENTRYPOINT: &str = "modify_write";
pub const SHARE_RO_ENTRYPOINT: &str = "share_ro";
pub const SHARE_W_ENTRYPOINT: &str = "share_w";
pub const CONTRACT_HASH_NAME: &str = "contract_hash";
const CONTRACT_PACKAGE_HASH_NAME: &str = "package_hash_name";
pub const DEFAULT_DICTIONARY_NAME: &str = "Default Key";
pub const DEFAULT_DICTIONARY_VALUE: &str = "Default Value";
pub const DICTIONARY_REF: &str = "new_dictionary";
pub const MALICIOUS_KEY_NAME: &str = "invalid dictionary name";

#[no_mangle]
fn modify_write() {
    // Preserve for further modifications
    let dictionary_uref = match runtime::get_key(DICTIONARY_NAME) {
        Some(key) => key.into_uref().unwrap_or_revert(),
        None => runtime::revert(ApiError::GetKey),
    };

    // Appends " Hello, world!" to a [66; 32] dictionary with spaces trimmed.
    // Two runs should yield value "Hello, world! Hello, world!" read from dictionary
    let mut res: String = storage::dictionary_get(dictionary_uref, DICTIONARY_PUT_KEY)
        .unwrap_or_default()
        .unwrap_or_default();

    res.push_str(HELLO_PREFIX);
    // Write "Hello, "
    storage::dictionary_put(dictionary_uref, DICTIONARY_PUT_KEY, res);

    // Read (this should exercise cache)
    let mut res: String = storage::dictionary_get(dictionary_uref, DICTIONARY_PUT_KEY)
        .unwrap_or_revert()
        .unwrap_or_revert();
    // Append
    res.push_str(WORLD_SUFFIX);
    // Write
    storage::dictionary_put(dictionary_uref, DICTIONARY_PUT_KEY, res.trim().to_string());
}

fn get_dictionary_uref() -> URef {
    let key = runtime::get_key(DICTIONARY_NAME).unwrap_or_revert();
    key.into_uref().unwrap_or_revert()
}

#[no_mangle]
fn share_ro() {
    let uref_ro = get_dictionary_uref().into_read();
    runtime::ret(CLValue::from_t(uref_ro).unwrap_or_revert())
}

#[no_mangle]
fn share_w() {
    let uref_w = get_dictionary_uref().into_write();
    runtime::ret(CLValue::from_t(uref_w).unwrap_or_revert())
}

pub fn delegate() {
    // Empty key name is invalid
    assert!(storage::new_dictionary("").is_err());
    // Assert that we don't have this key yet
    assert!(!runtime::has_key(MALICIOUS_KEY_NAME));
    // Create and put a new dictionary in named keys
    storage::new_dictionary(MALICIOUS_KEY_NAME).unwrap();
    // Can't do it twice
    assert!(storage::new_dictionary(MALICIOUS_KEY_NAME).is_err());

    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntryPoint::new(
        MODIFY_WRITE_ENTRYPOINT,
        Vec::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        SHARE_RO_ENTRYPOINT,
        Vec::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        SHARE_W_ENTRYPOINT,
        Vec::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    let named_keys = {
        let uref = {
            let dictionary_uref = storage::new_dictionary(DICTIONARY_REF).unwrap_or_revert();
            assert_eq!(
                dictionary_uref.access_rights() & AccessRights::READ_ADD_WRITE,
                AccessRights::READ_ADD_WRITE
            );

            storage::dictionary_put(
                dictionary_uref,
                DEFAULT_DICTIONARY_NAME,
                DEFAULT_DICTIONARY_VALUE,
            );
            dictionary_uref
        };
        let mut named_keys = NamedKeys::new();
        named_keys.insert(DICTIONARY_NAME.to_string(), uref.into());
        named_keys
    };

    let (contract_hash, _version) = storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(CONTRACT_PACKAGE_HASH_NAME.to_string()),
        None,
    );
    runtime::put_key(CONTRACT_HASH_NAME, contract_hash.into());
}
