#![no_std]
#![no_main]

use casperlabs_contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    account::{AccountHash, ActionType, AddKeyFailure, Weight},
    ApiError,
};

const ARG_KEY_MANAGEMENT_THRESHOLD: &str = "key_management_threshold";
const ARG_DEPLOY_THRESHOLD: &str = "deploy_threshold";

#[no_mangle]
pub extern "C" fn call() {
    match account::add_associated_key(AccountHash::new([123; 32]), Weight::new(100)) {
        Err(AddKeyFailure::DuplicateKey) => {}
        Err(_) => runtime::revert(ApiError::User(50)),
        Ok(_) => {}
    };

    let key_management_threshold: Weight = runtime::get_named_arg(ARG_KEY_MANAGEMENT_THRESHOLD);
    let deploy_threshold: Weight = runtime::get_named_arg(ARG_DEPLOY_THRESHOLD);

    if key_management_threshold != Weight::new(0) {
        account::set_action_threshold(ActionType::KeyManagement, key_management_threshold)
            .unwrap_or_revert()
    }

    if deploy_threshold != Weight::new(0) {
        account::set_action_threshold(ActionType::Deployment, deploy_threshold).unwrap_or_revert()
    }
}
