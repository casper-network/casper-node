#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::account::{ActionType, Weight};

const ARG_KEY_MANAGEMENT_THRESHOLD: &str = "key_management_threshold";
const ARG_DEPLOY_THRESHOLD: &str = "deploy_threshold";

#[no_mangle]
pub extern "C" fn call() {
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
