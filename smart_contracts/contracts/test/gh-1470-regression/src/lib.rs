#![no_std]

extern crate alloc;

use alloc::string::String;

use casper_types::U512;

pub const GROUP_LABEL: &str = "group_label";
pub const GROUP_UREF_NAME: &str = "group_uref";
pub const CONTRACT_HASH_NAME: &str = "contract_hash";
pub const CONTRACT_PACKAGE_HASH_NAME: &str = "contract_package_hash";
pub const RESTRICTED_DO_NOTHING_ENTRYPOINT: &str = "restricted_do_nothing_contract";

pub const ARG1: &str = "arg1";
pub type Arg1Type = String;

pub const ARG2: &str = "arg2";
pub type Arg2Type = U512;

pub const ARG3: &str = "arg3";
pub type Arg3Type = Option<u64>;
