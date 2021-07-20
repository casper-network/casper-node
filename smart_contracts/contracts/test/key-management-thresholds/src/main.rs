#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::{
        AccountHash, ActionType, AddKeyFailure, RemoveKeyFailure, SetThresholdFailure,
        UpdateKeyFailure, Weight,
    },
    ApiError,
};

const ARG_STAGE: &str = "stage";
#[no_mangle]
pub extern "C" fn call() {
    let stage: String = runtime::get_named_arg(ARG_STAGE);

    if stage == "init" {
        // executed with weight >= 1
        account::add_associated_key(AccountHash::new([42; 32]), Weight::new(100))
            .unwrap_or_revert();
        // this key will be used to test permission denied when removing keys with low
        // total weight
        account::add_associated_key(AccountHash::new([43; 32]), Weight::new(1)).unwrap_or_revert();
        account::add_associated_key(AccountHash::new([1; 32]), Weight::new(1)).unwrap_or_revert();
        account::set_action_threshold(ActionType::KeyManagement, Weight::new(101))
            .unwrap_or_revert();
    } else if stage == "test-permission-denied" {
        // Has to be executed with keys of total weight < 255
        match account::add_associated_key(AccountHash::new([44; 32]), Weight::new(1)) {
            Ok(_) => runtime::revert(ApiError::User(200)),
            Err(AddKeyFailure::PermissionDenied) => {}
            Err(_) => runtime::revert(ApiError::User(201)),
        }

        match account::update_associated_key(AccountHash::new([43; 32]), Weight::new(2)) {
            Ok(_) => runtime::revert(ApiError::User(300)),
            Err(UpdateKeyFailure::PermissionDenied) => {}
            Err(_) => runtime::revert(ApiError::User(301)),
        }
        match account::remove_associated_key(AccountHash::new([43; 32])) {
            Ok(_) => runtime::revert(ApiError::User(400)),
            Err(RemoveKeyFailure::PermissionDenied) => {}
            Err(_) => runtime::revert(ApiError::User(401)),
        }

        match account::set_action_threshold(ActionType::KeyManagement, Weight::new(255)) {
            Ok(_) => runtime::revert(ApiError::User(500)),
            Err(SetThresholdFailure::PermissionDeniedError) => {}
            Err(_) => runtime::revert(ApiError::User(501)),
        }
    } else if stage == "test-key-mgmnt-succeed" {
        // Has to be executed with keys of total weight >= 254
        account::add_associated_key(AccountHash::new([44; 32]), Weight::new(1)).unwrap_or_revert();
        // Updates [43;32] key weight created in init stage
        account::update_associated_key(AccountHash::new([44; 32]), Weight::new(2))
            .unwrap_or_revert();
        // Removes [43;32] key created in init stage
        account::remove_associated_key(AccountHash::new([44; 32])).unwrap_or_revert();
        // Sets action threshold
        account::set_action_threshold(ActionType::KeyManagement, Weight::new(100))
            .unwrap_or_revert();
    } else {
        runtime::revert(ApiError::User(1))
    }
}
