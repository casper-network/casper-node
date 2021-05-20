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

pub const LOCAL_KEY_NAME: &str = "local";
pub const WRITE_LOCAL_KEY: [u8; 32] = [66u8; 32];
pub const HELLO_PREFIX: &str = " Hello, ";
pub const WORLD_SUFFIX: &str = "world!";
pub const MODIFY_WRITE_ENTRYPOINT: &str = "modify_write";
pub const SHARE_RO_ENTRYPOINT: &str = "share_ro";
pub const SHARE_W_ENTRYPOINT: &str = "share_w";
pub const CONTRACT_HASH_NAME: &str = "contract_hash";
const CONTRACT_PACKAGE_HASH_NAME: &str = "package_hash_name";
pub const DEFAULT_LOCAL_KEY_NAME: &str = "Default Key";
pub const DEFAULT_LOCAL_KEY_VALUE: &str = "Default Value";

#[no_mangle]
fn modify_write() {
    // Preserve for further modifications
    let local = match runtime::get_key(LOCAL_KEY_NAME) {
        Some(key) => key.into_uref().unwrap_or_revert(),
        None => runtime::revert(ApiError::GetKey),
    };

    // Appends " Hello, world!" to a [66; 32] local key with spaces trimmed.
    // Two runs should yield value "Hello, world! Hello, world!"
    // read from local state
    let mut res: String = storage::read_local(local, WRITE_LOCAL_KEY)
        .unwrap_or_default()
        .unwrap_or_default();

    res.push_str(HELLO_PREFIX);
    // Write "Hello, "
    storage::write_local(local, WRITE_LOCAL_KEY, res);

    // Read (this should exercise cache)
    let mut res: String = storage::read_local(local, WRITE_LOCAL_KEY)
        .unwrap_or_revert()
        .unwrap_or_revert();
    // Append
    res.push_str(WORLD_SUFFIX);
    // Write
    storage::write_local(local, WRITE_LOCAL_KEY, res.trim().to_string());
}

fn get_local_key_uref() -> URef {
    let key = runtime::get_key(LOCAL_KEY_NAME).unwrap_or_revert();
    key.into_uref().unwrap_or_revert()
}

#[no_mangle]
fn share_ro() {
    let uref_ro = get_local_key_uref().into_read();
    runtime::ret(CLValue::from_t(uref_ro).unwrap_or_revert())
}

#[no_mangle]
fn share_w() {
    let uref_w = get_local_key_uref().into_write();
    runtime::ret(CLValue::from_t(uref_w).unwrap_or_revert())
}

pub fn delegate() {
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
            let local_uref = storage::create_local().unwrap_or_revert();
            assert_eq!(
                local_uref.access_rights() & AccessRights::READ_ADD_WRITE,
                AccessRights::READ_ADD_WRITE
            );

            storage::write_local(local_uref, DEFAULT_LOCAL_KEY_NAME, DEFAULT_LOCAL_KEY_VALUE);
            local_uref
        };
        let mut named_keys = NamedKeys::new();
        named_keys.insert(LOCAL_KEY_NAME.to_string(), uref.into());
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
