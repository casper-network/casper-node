#![no_std]
#![no_main]

use casperlabs_contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::account::{AccountHash, ActionType, Weight};

const ARG_KEY_MANAGEMENT_THRESHOLD: &str = "key_management_threshold";
const ARG_DEPLOYMENT_THRESHOLD: &str = "deployment_threshold";

#[no_mangle]
pub extern "C" fn call() {
    account::add_associated_key(AccountHash::new([123; 32]), Weight::new(254)).unwrap_or_revert();
    let key_management_threshold: Weight = runtime::get_named_arg(ARG_KEY_MANAGEMENT_THRESHOLD);
    let deployment_threshold: Weight = runtime::get_named_arg(ARG_DEPLOYMENT_THRESHOLD);

    account::set_action_threshold(ActionType::KeyManagement, key_management_threshold)
        .unwrap_or_revert();
    account::set_action_threshold(ActionType::Deployment, deployment_threshold).unwrap_or_revert();
}
