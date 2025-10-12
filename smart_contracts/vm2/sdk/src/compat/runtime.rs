use borsh::BorshDeserialize;
use casper_executor_wasm_common::flags::ReturnFlags;

use crate::{
    casper::{self, call_result_from_code, casper_ffi},
    compat::types::{CLValue, RuntimeArgs},
    types::{CallError, EmitFunctionOption},
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
    let env_info = casper::get_env_info().expect("expected get_env_info to yield data");
    env_info.caller_addr
}

pub fn get_blocktime() -> u64 {
    let env_info = casper::get_env_info().expect("expected get_env_info to yield data");
    env_info.block_time
}

pub fn get_block_height() -> u64 {
    let env_info = casper::get_env_info().expect("expected get_env_info to yield data");
    env_info.block_height
}

pub fn get_parent_block_hash() -> [u8; 32] {
    let env_info = casper::get_env_info().expect("expected get_env_info to yield data");
    env_info.parent_block_hash
}

pub fn get_protocol_version() -> (u32, u32, u32) {
    let env_info = casper::get_env_info().expect("expected get_env_info to yield data");
    (
        env_info.protocol_version_major,
        env_info.protocol_version_minor,
        env_info.protocol_version_patch,
    )
}

#[inline]
pub fn get_immediate_caller() -> [u8; 32] {
    let env_info = casper::get_env_info().expect("expected get_env_info to yield data");
    env_info.caller_addr
}

#[inline]
pub fn emit_message(topic_name: &str, message: &[u8]) -> Result<(), CallError> {
    let args = (topic_name, message);
    let arg_bytes = borsh::to_vec(&args).expect("Expected borsh to work");

    let (_, result_code) = casper_ffi(EmitFunctionOption::Native.into(), &arg_bytes);
    call_result_from_code(result_code)
}

#[inline]
pub fn print(text: &str) {
    let _ = casper::print(text);
}
