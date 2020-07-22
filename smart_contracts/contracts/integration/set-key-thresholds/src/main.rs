#![no_std]
#![no_main]

use casperlabs_contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    account::{ActionType, Weight},
    ApiError,
};

#[repr(u16)]
enum Error {
    SetKeyManagementThreshold = 100,
    SetDeploymentThreshold = 200,
}

impl Into<ApiError> for Error {
    fn into(self) -> ApiError {
        ApiError::User(self as u16)
    }
}

const ARG_KM_WEIGHT: &str = "km_weight";
const ARG_DEP_WEIGHT: &str = "dep_weight";

#[no_mangle]
pub extern "C" fn call() {
    let km_weight: u32 = runtime::get_named_arg(ARG_KM_WEIGHT);
    let dep_weight: u32 = runtime::get_named_arg(ARG_DEP_WEIGHT);
    let key_management_threshold = Weight::new(km_weight as u8);
    let deploy_threshold = Weight::new(dep_weight as u8);

    if key_management_threshold != Weight::new(0) {
        account::set_action_threshold(ActionType::KeyManagement, key_management_threshold)
            .unwrap_or_revert_with(Error::SetKeyManagementThreshold);
    }

    if deploy_threshold != Weight::new(0) {
        account::set_action_threshold(ActionType::Deployment, deploy_threshold)
            .unwrap_or_revert_with(Error::SetDeploymentThreshold);
    }
}
