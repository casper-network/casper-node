#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::{ContractHash, Parameters},
    ApiError, CLType, CLValue, ContractVersion, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Phase, RuntimeArgs,
};

const ARG_TARGET: &str = "target_contract";
const NOOP_EXT: &str = "noop_ext";
const GET_PHASE_EXT: &str = "get_phase_ext";

#[repr(u16)]
enum CustomError {
    UnexpectedPhaseInline = 0,
    UnexpectedPhaseSub = 1,
}

#[no_mangle]
pub extern "C" fn get_phase_ext() {
    let phase = runtime::get_phase();
    runtime::ret(CLValue::from_t(phase).unwrap_or_revert())
}

#[no_mangle]
pub extern "C" fn noop_ext() {
    runtime::ret(CLValue::from_t(()).unwrap_or_revert())
}

fn store() -> (ContractHash, ContractVersion) {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point_1 = EntryPoint::new(
            NOOP_EXT,
            Parameters::default(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(entry_point_1);

        let entry_point_2 = EntryPoint::new(
            GET_PHASE_EXT,
            Parameters::default(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(entry_point_2);

        entry_points
    };
    storage::new_contract(entry_points, None, None, None)
}

#[no_mangle]
pub extern "C" fn call() {
    const NOOP_EXT: &str = "noop_ext";
    const GET_PHASE_EXT: &str = "get_phase_ext";

    let method_name: String = runtime::get_named_arg(ARG_TARGET);
    match method_name.as_str() {
        "no-subcall" => {
            let phase = runtime::get_phase();
            if phase != Phase::Session {
                runtime::revert(ApiError::User(CustomError::UnexpectedPhaseInline as u16))
            }
        }
        "do-nothing" => {
            let (reference, _contract_version) = store();
            runtime::call_contract(reference, NOOP_EXT, RuntimeArgs::default())
        }
        "do-something" => {
            let (reference, _contract_version) = store();
            let phase: Phase =
                runtime::call_contract(reference, GET_PHASE_EXT, RuntimeArgs::default());
            if phase != Phase::Session {
                runtime::revert(ApiError::User(CustomError::UnexpectedPhaseSub as u16))
            }
        }
        _ => {}
    }
}
