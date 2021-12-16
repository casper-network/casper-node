#![no_std]
#![no_main]

extern crate alloc;

use alloc::{collections::BTreeSet, string::ToString};
use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    account::AccountHash, contracts::Parameters, ApiError, CLType, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints,
};

const ROLE_A_KEYS: [AccountHash; 3] = [
    AccountHash::new([1; 32]),
    AccountHash::new([2; 32]),
    AccountHash::new([3; 32]),
];
const ROLE_B_KEYS: [AccountHash; 3] = [
    AccountHash::new([4; 32]),
    AccountHash::new([5; 32]),
    AccountHash::new([6; 32]),
];

const ACCESS_KEY: &str = "access_key";
const CONTRACT_KEY: &str = "contract";
const ENTRYPOINT_A: &str = "entrypoint_a";
const ENTRYPOINT_B: &str = "entrypoint_b";
const CONTRACT_PACKAGE_KEY: &str = "contract_package";

#[repr(u16)]
enum UserError {
    /// Deploy was signed using a key that does not belong to a role.
    PermissionDenied = 0,
}

impl From<UserError> for ApiError {
    fn from(user_error: UserError) -> Self {
        ApiError::User(user_error as u16)
    }
}

/// Checks if at least one of provided authorization keys belongs to a role defined as a slice of
/// `AccountHash`es.
fn has_role_access_to(role_keys: &[AccountHash]) -> bool {
    let authorization_keys = runtime::list_authorization_keys();
    let role_b_keys: BTreeSet<AccountHash> = role_keys.iter().copied().collect();
    (&authorization_keys).intersection(&role_b_keys).count() > 0
}

#[no_mangle]
pub extern "C" fn entrypoint_a() {
    if !has_role_access_to(&ROLE_A_KEYS) {
        // None of the authorization keys used to sign this deploy matched ROLE_A
        runtime::revert(UserError::PermissionDenied)
    }

    // Restricted code
}

#[no_mangle]
pub extern "C" fn entrypoint_b() {
    if !has_role_access_to(&ROLE_B_KEYS) {
        // None of the authorization keys used to sign this deploy matched ROLE_B
        runtime::revert(UserError::PermissionDenied)
    }

    // Restricted code
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entrypoint_a = EntryPoint::new(
            ENTRYPOINT_A,
            Parameters::default(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        let entrypoint_b = EntryPoint::new(
            ENTRYPOINT_B,
            Parameters::default(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(entrypoint_a);
        entry_points.add_entry_point(entrypoint_b);

        entry_points
    };

    let (contract_hash, _version) = storage::new_contract(
        entry_points,
        None,
        Some(CONTRACT_PACKAGE_KEY.to_string()),
        Some(ACCESS_KEY.to_string()),
    );

    runtime::put_key(CONTRACT_KEY, contract_hash.into());
}
