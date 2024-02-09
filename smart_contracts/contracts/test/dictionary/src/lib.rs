#![no_std]

extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::mem::MaybeUninit;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    addressable_entity::{EntityKindTag, NamedKeys},
    api_error,
    bytesrepr::ToBytes,
    AccessRights, ApiError, CLType, CLValue, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Key, URef,
};

pub const DICTIONARY_NAME: &str = "local";
pub const DICTIONARY_PUT_KEY: &str = "item_key";
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
pub const INVALID_PUT_DICTIONARY_ITEM_KEY_ENTRYPOINT: &str = "invalid_put_dictionary_item_key";
pub const INVALID_GET_DICTIONARY_ITEM_KEY_ENTRYPOINT: &str = "invalid_get_dictionary_item_key";

#[no_mangle]
fn modify_write() {
    // Preserve for further modifications
    let dictionary_seed_uref = match runtime::get_key(DICTIONARY_NAME) {
        Some(key) => key.into_uref().unwrap_or_revert(),
        None => runtime::revert(ApiError::GetKey),
    };

    // Appends " Hello, world!" to a [66; 32] dictionary with spaces trimmed.
    // Two runs should yield value "Hello, world! Hello, world!" read from dictionary
    let mut res: String = storage::dictionary_get(dictionary_seed_uref, DICTIONARY_PUT_KEY)
        .unwrap_or_default()
        .unwrap_or_default();

    res.push_str(HELLO_PREFIX);
    // Write "Hello, "
    storage::dictionary_put(dictionary_seed_uref, DICTIONARY_PUT_KEY, res);

    // Read (this should exercise cache)
    let mut res: String = storage::dictionary_get(dictionary_seed_uref, DICTIONARY_PUT_KEY)
        .unwrap_or_revert()
        .unwrap_or_revert();
    // Append
    res.push_str(WORLD_SUFFIX);
    // Write
    storage::dictionary_put(
        dictionary_seed_uref,
        DICTIONARY_PUT_KEY,
        res.trim().to_string(),
    );
}

fn get_dictionary_seed_uref() -> URef {
    let key = runtime::get_key(DICTIONARY_NAME).unwrap_or_revert();
    key.into_uref().unwrap_or_revert()
}

#[no_mangle]
fn share_ro() {
    let uref_ro = get_dictionary_seed_uref().into_read();
    runtime::ret(CLValue::from_t(uref_ro).unwrap_or_revert())
}

#[no_mangle]
fn share_w() {
    let uref_w = get_dictionary_seed_uref().into_write();
    runtime::ret(CLValue::from_t(uref_w).unwrap_or_revert())
}

fn to_ptr<T: ToBytes>(t: T) -> (*const u8, usize, Vec<u8>) {
    let bytes = t.into_bytes().unwrap_or_revert();
    let ptr = bytes.as_ptr();
    let size = bytes.len();
    (ptr, size, bytes)
}

#[no_mangle]
fn invalid_put_dictionary_item_key() {
    let dictionary_seed_uref = get_dictionary_seed_uref();
    let (uref_ptr, uref_size, _bytes1) = to_ptr(dictionary_seed_uref);

    let bad_dictionary_item_key = alloc::vec![0, 159, 146, 150];
    let bad_dictionary_item_key_ptr = bad_dictionary_item_key.as_ptr();
    let bad_dictionary_item_key_size = bad_dictionary_item_key.len();

    let cl_value = CLValue::unit();
    let (cl_value_ptr, cl_value_size, _bytes) = to_ptr(cl_value);

    let result = unsafe {
        let ret = test_ffi::casper_dictionary_put(
            uref_ptr,
            uref_size,
            bad_dictionary_item_key_ptr,
            bad_dictionary_item_key_size,
            cl_value_ptr,
            cl_value_size,
        );
        api_error::result_from(ret)
    };

    result.unwrap_or_revert()
}

#[no_mangle]
fn invalid_get_dictionary_item_key() {
    let dictionary_seed_uref = get_dictionary_seed_uref();
    let (uref_ptr, uref_size, _bytes1) = to_ptr(dictionary_seed_uref);

    let bad_dictionary_item_key = alloc::vec![0, 159, 146, 150];
    let bad_dictionary_item_key_ptr = bad_dictionary_item_key.as_ptr();
    let bad_dictionary_item_key_size = bad_dictionary_item_key.len();

    let _value_size = {
        let mut value_size = MaybeUninit::uninit();
        let ret = unsafe {
            test_ffi::casper_dictionary_get(
                uref_ptr,
                uref_size,
                bad_dictionary_item_key_ptr,
                bad_dictionary_item_key_size,
                value_size.as_mut_ptr(),
            )
        };
        match api_error::result_from(ret) {
            Ok(_) => unsafe { value_size.assume_init() },
            Err(e) => runtime::revert(e),
        }
    };
}

mod test_ffi {
    extern "C" {
        pub fn casper_dictionary_put(
            uref_ptr: *const u8,
            uref_size: usize,
            key_ptr: *const u8,
            key_size: usize,
            value_ptr: *const u8,
            value_size: usize,
        ) -> i32;

        pub fn casper_dictionary_get(
            uref_ptr: *const u8,
            uref_size: usize,
            key_bytes_ptr: *const u8,
            key_bytes_size: usize,
            output_size: *mut usize,
        ) -> i32;
    }
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
        EntryPointType::AddressableEntity,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        SHARE_RO_ENTRYPOINT,
        Vec::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        SHARE_W_ENTRYPOINT,
        Vec::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        INVALID_PUT_DICTIONARY_ITEM_KEY_ENTRYPOINT,
        Vec::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        INVALID_GET_DICTIONARY_ITEM_KEY_ENTRYPOINT,
        Vec::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
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

    let (entity_hash, _version) = storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(CONTRACT_PACKAGE_HASH_NAME.to_string()),
        None,
    );

    let entity_key = Key::addressable_entity_key(EntityKindTag::SmartContract, entity_hash);

    runtime::put_key(CONTRACT_HASH_NAME, entity_key);
}
