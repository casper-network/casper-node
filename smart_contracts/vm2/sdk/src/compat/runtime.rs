use borsh::BorshDeserialize;
use casper_executor_wasm_common::{error::HostResult, flags::ReturnFlags};

use crate::{
    casper,
    compat::types::{CLValue, RuntimeArgs},
};

fn get_runtime_args() -> RuntimeArgs {
    let arg_bytes = casper::copy_input();
    borsh::from_slice(&arg_bytes).expect("Failed to deserialize runtime arguments")
}

pub fn ret(value: CLValue) {
    let bytes = borsh::to_vec(&value).expect("Failed to serialize return value");
    casper::ret(ReturnFlags::empty(), Some(&bytes));
}

pub fn get_named_arg<T: BorshDeserialize>(name: &str) -> T {
    let runtime_args = get_runtime_args();
    let arg = runtime_args
        .get(name)
        .unwrap_or_else(|| panic!("Named argument '{}' not found", name));
    let value: T = borsh::from_slice(arg.inner_bytes())
        .unwrap_or_else(|_| panic!("Named argument '{}' has wrong type", name));
    value
}

pub fn try_get_named_arg<T: BorshDeserialize>(name: &str) -> Option<T> {
    let runtime_args = get_runtime_args();
    runtime_args
        .get(name)
        .and_then(|arg| borsh::from_slice(arg.inner_bytes()).ok())
}

pub fn get_caller() -> [u8; 32] {
    let env_info = casper::get_env_info();
    env_info.caller_addr
}

pub fn get_blocktime() -> u64 {
    let env_info = casper::get_env_info();
    env_info.block_time
}

pub fn get_block_height() -> u64 {
    let env_info = casper::get_env_info();
    env_info.block_height
}

pub fn get_parent_block_hash() -> [u8; 32] {
    let env_info = casper::get_env_info();
    env_info.parent_block_hash
}

pub fn get_protocol_version() -> (u32, u32, u32) {
    let env_info = casper::get_env_info();
    (
        env_info.protocol_version_major,
        env_info.protocol_version_minor,
        env_info.protocol_version_patch,
    )
}

#[inline]
pub fn get_immediate_caller() -> [u8; 32] {
    let env_info = casper::get_env_info();
    env_info.caller_addr
}

#[inline]
pub fn emit_message(topic_name: &str, message: &[u8]) -> Result<(), HostResult> {
    casper::emit(topic_name, message)
}

#[inline]
pub fn print(text: &str) {
    casper::print(text);
}
